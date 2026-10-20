mod acts;
mod cache;
mod collections;
mod contributions;
mod database;
mod events;
mod files;
mod folder;
mod hooks;
mod interfaces;
mod interop;
mod limits;
mod manifest;
mod model;
mod needs;
mod net;
mod overlay;
mod panels;
mod permissions;
mod pick;
mod reads;
mod reload;
mod state;
mod steer;
mod stopped;
mod storage;
mod tools;
mod tree;
mod uninstall;

use wasmtime::component::Component;

use crate::engine::Host;
use crate::fault::Fault;
use crate::fixture;
use crate::host::Logged;
use crate::install::{self, Body, Why};
use crate::instance::Instance;
use crate::interface::VERSION;
use crate::interface::exports::hamlet::plugin::plugin::{
    Incoming, Intent,
};
use crate::interface::hamlet::plugin::host::Level;
use crate::limits::Limits;

fn load(module: &str) -> Result<Instance, Fault> {
    let limits = Limits::default().patient();
    let host = Host::new(&limits).unwrap();
    let component =
        Component::new(&host.0, fixture::component(module))
            .map_err(Fault::load)?;
    Instance::new(&host.0, &component, &limits)
}

#[test]
fn step_answers_from_its_inbox_and_log_reaches_the_host() {
    let limits = Limits::default().patient();
    let mut plugin = load(&fixture::echo()).unwrap();
    plugin.entry(&limits).unwrap();
    let inbox =
        [Incoming::Tick(1), Incoming::Tick(2), Incoming::Tick(3)];

    let out = plugin.step(&limits, &inbox).unwrap();

    assert_eq!(out.intents, [Intent::LogLine("hel".into())]);
    assert_eq!(
        out.logs,
        [Logged {
            level: Level::Info,
            message: "logged".into()
        }]
    );
}

#[test]
fn a_call_that_never_returns_runs_out_of_time() {
    let limits = Limits {
        call_fuel: u64::MAX >> 1,
        ..Limits::default()
    };
    let mut plugin = load(&fixture::spin()).unwrap();
    assert_eq!(
        plugin.step(&limits, &[]).map(drop),
        Err(Fault::OutOfTime)
    );
}

#[test]
fn a_call_that_never_returns_runs_out_of_fuel() {
    let limits = Limits::default().patient();
    let mut plugin = load(&fixture::spin()).unwrap();
    assert_eq!(
        plugin.step(&limits, &[]).map(drop),
        Err(Fault::OutOfFuel)
    );
}

#[test]
fn memory_that_grows_without_end_is_stopped() {
    let limits = Limits::default().patient();
    let mut plugin = load(&fixture::grow()).unwrap();
    let fault = plugin.step(&limits, &[]).map(drop).unwrap_err();
    assert!(matches!(fault, Fault::OutOfMemory(_)), "{fault}");
}

#[test]
fn strings_that_share_their_bytes_are_not_copied_past_the_cap() {
    let limits = Limits::default().patient();
    let mut plugin = load(&fixture::hoarding()).unwrap();
    let fault = plugin.step(&limits, &[]).map(drop).unwrap_err();
    assert!(
        matches!(&fault, Fault::Trap(why) if why.contains("too much data")),
        "{fault}"
    );
}

#[test]
fn an_import_the_host_does_not_provide_fails_to_load() {
    let limits = Limits::default();
    let host = Host::new(&limits).unwrap();
    let component =
        Component::new(&host.0, fixture::greedy()).unwrap();
    let Err(Fault::Load(why)) =
        Instance::new(&host.0, &component, &limits).map(drop)
    else {
        panic!("it loaded");
    };
    assert!(why.contains("greedy:thing/stuff"), "{why}");
}

#[test]
fn a_build_for_another_interface_says_which() {
    let limits = Limits::default();
    let host = Host::new(&limits).unwrap();
    let component =
        Component::new(&host.0, fixture::outdated()).unwrap();
    let Err(Fault::Load(why)) =
        Instance::new(&host.0, &component, &limits).map(drop)
    else {
        panic!("it loaded");
    };
    assert_eq!(
        why,
        "built for interface 0.0.1, this Hamlet speaks 0.1.0"
    );
}

#[test]
fn a_folder_that_is_not_a_plugin_is_turned_down() {
    let plugins = std::env::temp_dir().join(format!(
        "hamlet_wasm_install_scan_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&plugins);
    let manifest = |id: &str, interface: &str| {
        format!(
            "id = \"{id}\"\nname = \"X\"\nversion = \
             \"0.1.0\"\ninterface = \"{interface}\"\n"
        )
    };
    let put = |folder: &str, manifest: Option<String>| {
        let dir = plugins.join(folder);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("plugin.wasm"), b"").unwrap();
        if let Some(text) = manifest {
            std::fs::write(dir.join("manifest.toml"), text).unwrap();
        }
    };
    put("good", Some(manifest("good", VERSION)));
    put("bare", None);
    put("garbled", Some("id = ".into()));
    put("renamed", Some(manifest("other", VERSION)));
    put("old", Some(manifest("old", "0.0.1")));
    let limits = Limits::default();
    let long = manifest("long", VERSION)
        + &"#".repeat(limits.manifest_bytes)
        + "\n";
    put("long", Some(long));
    let data = |folder: &str, kind: &str| {
        let dir = plugins.join(folder);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("panel.ui"), b"root = Column([])")
            .unwrap();
        std::fs::write(
            dir.join("manifest.toml"),
            format!(
                "id = \"{folder}\"\nname = \"X\"\nversion = \
                 \"0.1.0\"\nkind = \"{kind}\"\n"
            ),
        )
        .unwrap();
    };
    data("greeter", "data");
    data("odd", "script");

    let found = install::scan(&plugins, &limits);

    let why = |folder: &str| {
        found
            .iter()
            .filter_map(|r| r.as_ref().err())
            .find(|r| r.folder == folder)
            .map(|r| &r.why)
    };
    let accepted = found
        .iter()
        .filter_map(|r| r.as_ref().ok())
        .map(|plugin| (plugin.manifest.id.as_str(), &plugin.body))
        .collect::<Vec<_>>();
    assert!(matches!(
        accepted[..],
        [("good", Body::Wasm(_)), ("greeter", Body::Data(_))]
    ));
    assert!(matches!(why("odd"), Some(Why::BadManifest(_))));
    assert_eq!(why("bare"), Some(&Why::Missing("manifest.toml")));
    assert!(matches!(why("garbled"), Some(Why::BadManifest(_))));
    assert!(matches!(why("renamed"), Some(Why::WrongFolder { .. })));
    assert!(matches!(why("old"), Some(Why::Interface { .. })));
    assert!(matches!(
        why("long"),
        Some(Why::BadManifest(why)) if why.contains("more than")
    ));
}
