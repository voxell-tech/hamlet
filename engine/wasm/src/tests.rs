//! Plugins written as core modules in text, made into components
//! against the WIT file, so no test needs a WASM toolchain.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use bevy::app::{App, TaskPoolPlugin};
use bevy::ecs::entity::Entity;
use bevy::ecs::query::With;
use bevy::time::TimePlugin;
use hamlet_engine_api::{
    API_VERSION, Capability, PluginId, PluginLimits,
};
use hamlet_engine_core::tuning::Tuning;
use hamlet_engine_core::{Bundle, Guards, Home, LogLevel};
use hamlet_engine_grants::{Admitted, Answer, Grants, Petition};
use hamlet_engine_plugins::Uninstall;
use wasmtime::component::Resource;
use wit_component::{ComponentEncoder, StringEncoding};
use wit_parser::Resolve;

use crate::add::check;
use crate::budget::Spending;
use crate::compiled;
use crate::folder::{FOLDER, MANIFEST, WASM};
use crate::host::{Host, WasmTuning};
use crate::instance::ui_bindings::hamlet::engine::ui::{
    HostNode, Kind, Property,
};
use crate::instance::{
    Caps, Fault, Instance, State, link, link_compiled,
};
use crate::nodes::NodeError;

/// What `run` does in a test plugin that counts its runs.
const COUNT: &str =
    "(global.set $count (i32.add (global.get $count) (i32.const 1)))";

/// The exports every test plugin has, with `declare` and `run` doing
/// what they are given. The return area at 0 points at an empty list,
/// which is no `Contribution`s; the one at 32 at the `Contribution`s
/// of one panel, "p", whose name is also at 64. The count, which
/// [`COUNT`] counts, is its state.
fn module(declare: &str, run: &str) -> String {
    format!(
        r#"(module
  (import "hamlet:engine/host@0.1.0" "log" (func $log (param i32 i32 i32)))
  (memory (export "memory") 1)
  (memory $more 1)
  (global $heap (mut i32) (i32.const 4096))
  (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32)
    (local $at i32)
    (local.set $at
      (i32.and
        (i32.add (global.get $heap) (i32.sub (local.get 2) (i32.const 1)))
        (i32.sub (i32.const 0) (local.get 2))))
    (global.set $heap (i32.add (local.get $at) (local.get 3)))
    (local.get $at))
  (data (i32.const 0) "\08\00\00\00\01\00\00\00\00")
  (data (i32.const 16) "\01\00\01p\01P\00\00")
  (data (i32.const 32) "\10\00\00\00\08\00\00\00")
  (data (i32.const 64) "p")
  (func (export "hamlet:engine/guest@0.1.0#declare") (result i32)
    {declare})
  (func (export "hamlet:engine/guest@0.1.0#receive") (param i32 i32))
  (func (export "hamlet:engine/guest@0.1.0#serve")
    (param i32 i32 i32 i32 i32 i32) (result i32)
    (i32.const 256))
  (global $count (mut i32) (i32.const 0))
  (func (export "hamlet:engine/guest@0.1.0#run") (param i32 i32)
    {run})
  (func (export "hamlet:engine/guest@0.1.0#save") (result i32)
    (i32.store (i32.const 128) (global.get $count))
    (i32.store (i32.const 96) (i32.const 128))
    (i32.store (i32.const 100) (i32.const 4))
    (i32.const 96))
  (func (export "hamlet:engine/guest@0.1.0#restore") (param i32 i32)
    (if (i32.eq (local.get 1) (i32.const 4))
      (then (global.set $count (i32.load (local.get 0)))))))"#
    )
}

/// The parts of WASI 0.2 a test plugin uses, and a world of the
/// plugin's with them.
const WASI: [&str; 5] = [
    "package wasi:io@0.2.0;
    interface error { resource error; }
    interface poll { resource pollable { block: func(); } }
    interface streams {
      use error.{error};
      variant stream-error { last-operation-failed(error), closed }
      resource output-stream {
        blocking-write-and-flush: func(contents: list<u8>) -> \
     result<_, stream-error>;
      }
    }",
    "package wasi:clocks@0.2.0;
    interface monotonic-clock {
      use wasi:io/poll@0.2.0.{pollable};
      now: func() -> u64;
      subscribe-duration: func(when: u64) -> pollable;
    }",
    "package wasi:random@0.2.0;
    interface random { get-random-u64: func() -> u64; }",
    "package wasi:cli@0.2.0;
    interface stdout {
      use wasi:io/streams@0.2.0.{output-stream};
      get-stdout: func() -> output-stream;
    }",
    "package test:system;
    world plugin {
      include hamlet:engine/plugin@0.1.0;
      import wasi:cli/stdout@0.2.0;
      import wasi:clocks/monotonic-clock@0.2.0;
      import wasi:random/random@0.2.0;
    }",
];

/// The component of the core module in `text`, with the imports of
/// [`WASI`] when `wasi`.
fn component(text: &str, wasi: bool) -> Vec<u8> {
    let mut resolve = Resolve::default();
    let mut package = resolve
        .push_str(
            "hamlet_engine.wit",
            include_str!("../../wit/hamlet_engine.wit"),
        )
        .expect("the WIT parses");
    if wasi {
        for text in WASI {
            package = resolve
                .push_str("wasi.wit", text)
                .expect("the WIT parses");
        }
    }
    let world = resolve
        .select_world(&[package], Some("plugin"))
        .expect("the world is there");
    let mut core = wat::parse_str(text).expect("the module parses");
    wit_component::embed_component_metadata(
        &mut core,
        &resolve,
        world,
        StringEncoding::UTF8,
        false,
    )
    .expect("the WIT embeds");
    ComponentEncoder::default()
        .module(&core)
        .expect("the module is one")
        .validate(true)
        .encode()
        .expect("the component encodes")
}

/// A plugin of four pages of memory, as the person may switch on.
fn small() -> PluginLimits {
    PluginLimits {
        memory: Some(4 * 65_536),
        ..PluginLimits::default()
    }
}

/// The plugin `component`, started within `limits` and the guards,
/// with its caps.
fn start(
    component: &[u8],
    limits: &PluginLimits,
    guards: &Guards,
) -> (Instance, Caps) {
    let tuning = WasmTuning::designed();
    let host = Host::new(&tuning).expect("the host starts");
    let linked = link(&host.0, component).expect("the plugin links");
    let caps = Caps::of(limits, guards, &tuning);
    let instance = Instance::new(&host.0, &linked, &caps)
        .expect("the plugin starts");
    (instance, caps)
}

/// The plugin whose `declare` does `declare`, made into a component.
fn declaring(declare: &str) -> Vec<u8> {
    component(&module(declare, COUNT), false)
}

#[test]
fn nodes_are_held_to_their_kinds_and_depth() {
    let guards = Guards {
        node_depth: 1,
        ..Guards::designed()
    };
    let caps = Caps::of(
        &PluginLimits::default(),
        &guards,
        &WasmTuning::designed(),
    );
    let mut state = State::new(&caps);
    let root = state.nodes.root().rep();
    let make = |state: &mut State, kind, parent: u32| {
        HostNode::new(state, kind, Resource::new_borrow(parent), 0)
            .map(|node| node.rep())
    };
    let set = |state: &mut State, node: u32, value| {
        HostNode::set(state, Resource::new_borrow(node), value, None)
    };
    let refused =
        |error: wasmtime::Error| match Fault::of(error, &caps) {
            Fault::Node(why) => why,
            fault => panic!("{fault}"),
        };

    let label = make(&mut state, Kind::Label, root).unwrap();
    let under_label = make(&mut state, Kind::Label, label);
    assert_eq!(refused(under_label.unwrap_err()), NodeError::Leaf);
    let placeholder = Property::Placeholder("A name".into());
    let wrong = set(&mut state, label, placeholder);
    assert_eq!(refused(wrong.unwrap_err()), NodeError::Property);
    let frame = make(&mut state, Kind::Frame, root).unwrap();
    let deep = make(&mut state, Kind::Label, frame);
    assert_eq!(refused(deep.unwrap_err()), NodeError::Depth);
}

#[test]
fn a_limit_holds_only_once_switched_on_and_a_hang_is_stopped() {
    let guards = Guards {
        hung: 0.05,
        ..Guards::designed()
    };
    let none = PluginLimits::default();
    let timed = PluginLimits {
        call_time: Some(1),
        ..PluginLimits::default()
    };
    // Two memories, each within four pages and together past them.
    let grow =
        "(drop (memory.grow $more (i32.const 3))) (i32.const 0)";
    let endless = "(loop $again (br $again)) (i32.const 0)";
    let cases = [
        (grow, small(), Err(Fault::Memory)),
        (grow, none, Ok(Vec::new())),
        (endless, timed, Err(Fault::Time)),
        (endless, none, Err(Fault::Hung(0))),
    ];
    for (declare, limits, wanted) in cases {
        let (mut instance, caps) =
            start(&declaring(declare), &limits, &guards);
        assert_eq!(instance.declare(&caps).0, wanted, "{declare}");
    }
}

#[test]
fn the_largest_limits_are_taken() {
    let limits = PluginLimits {
        memory: Some(u64::MAX),
        call_time: Some(u64::MAX),
        ..PluginLimits::default()
    };
    let (mut instance, caps) = start(
        &declaring("(i32.const 0)"),
        &limits,
        &Guards::designed(),
    );
    assert_eq!(instance.declare(&caps).0, Ok(Vec::new()));
}

#[test]
fn a_plugin_has_the_system_and_its_waits_end_with_the_call() {
    let imports = r#"(module
  (import "wasi:clocks/monotonic-clock@0.2.0" "now" (func $now (result i64)))
  (import "wasi:clocks/monotonic-clock@0.2.0" "subscribe-duration" (func $sleep (param i64) (result i32)))
  (import "wasi:io/poll@0.2.0" "[method]pollable.block" (func $block (param i32)))
  (import "wasi:random/random@0.2.0" "get-random-u64" (func $random (result i64)))
  (import "wasi:cli/stdout@0.2.0" "get-stdout" (func $stdout (result i32)))
  (import "wasi:io/streams@0.2.0" "[method]output-stream.blocking-write-and-flush" (func $write (param i32 i32 i32 i32)))"#;
    // Declaring, it reads the clock and a random number, then writes
    // a line of 2,000 bytes to stdout 200 times.
    let declare = "(local $out i32) (local $left i32)
        (drop (call $now))
        (drop (call $random))
        (memory.fill (i32.const 1024) (i32.const 97) (i32.const \
                   1999))
        (i32.store8 (i32.const 3023) (i32.const 10))
        (local.set $out (call $stdout))
        (local.set $left (i32.const 200))
        (loop $again
          (call $write (local.get $out) (i32.const 1024)
            (i32.const 2000) (i32.const 512))
          (local.set $left (i32.sub (local.get $left) (i32.const 1)))
          (br_if $again (local.get $left)))
        (i32.const 0)";
    // Running, it sleeps for a thousand seconds.
    let run = "(call $block (call $sleep (i64.const 1000000000000)))";
    let text = module(declare, run).replacen("(module", imports, 1);
    let timed = PluginLimits {
        call_time: Some(20),
        ..small()
    };
    let (mut instance, caps) =
        start(&component(&text, true), &timed, &Guards::designed());

    assert_eq!(instance.declare(&caps).0, Ok(Vec::new()));
    let logs = instance.take_logs();
    assert_eq!(logs.len(), 200);
    assert!(logs.iter().all(|(level, text)| {
        *level == LogLevel::Info && text.len() == 1999
    }));

    // The sleep ends with the call's time.
    let started = Instant::now();
    let _ = instance.run(&caps, "p");
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn a_reloaded_plugin_keeps_its_state() {
    let tuning = WasmTuning::designed();
    let host = Host::new(&tuning).expect("the host starts");
    let plugin = declaring("(i32.const 0)");
    let linked = link(&host.0, &plugin).expect("the plugin links");
    let caps = Caps::of(&small(), &Guards::designed(), &tuning);
    let mut old = Instance::new(&host.0, &linked, &caps)
        .expect("the plugin starts");
    for _ in 0..3 {
        assert_eq!(old.run(&caps, "count").0, Ok(()));
    }
    let mut spending = Spending::new(Duration::ZERO);
    let (mut fresh, declared) = old
        .reload(&host.0, &linked, &caps, &mut spending)
        .expect("it reloads");
    assert_eq!(declared, Vec::new());
    assert_eq!(fresh.save(&caps).0, Ok(3_u32.to_le_bytes().to_vec()));
}

#[test]
fn a_kept_component_is_one_of_its_bytes_or_compiled_again() {
    let tuning = WasmTuning::designed();
    let host = Host::new(&tuning).expect("the host starts");
    let kept = home("kept_compiled");
    let shipped = home("shipped_compiled");
    let caps = Caps::of(&small(), &Guards::designed(), &tuning);
    let runs = |plugin: &[u8]| {
        let component =
            compiled::component(&host.0, plugin, &kept, &shipped)
                .expect("it compiles");
        let linked =
            link_compiled(&host.0, &component).expect("it links");
        let mut instance = Instance::new(&host.0, &linked, &caps)
            .expect("it starts");
        instance.run(&caps, "count").0
    };
    let files = || fs::read_dir(&kept).expect("it is kept").count();
    let plugin = declaring("(i32.const 0)");
    assert_eq!(runs(&plugin), Ok(()));
    assert_eq!(runs(&plugin), Ok(()));
    assert_eq!(files(), 1);
    for entry in fs::read_dir(&kept).expect("it is kept").flatten() {
        fs::write(entry.path(), b"spoiled").expect("it is spoiled");
    }
    assert_eq!(runs(&plugin), Ok(()));
    let other = declaring("(i32.const 1)");
    assert_eq!(runs(&other), Ok(()));
    assert_eq!(files(), 1);
    // One compiled when the plugin was built is run as it is, and
    // nothing is kept for it.
    let built = home("built_plugin");
    fs::create_dir_all(&built).expect("the folder is made");
    let wasm = built.join("plugin.wasm");
    let shipped_plugin = declaring("(i32.const 2)");
    fs::write(&wasm, &shipped_plugin).expect("it is written");
    crate::precompile(&wasm, &shipped).expect("it compiles");
    assert_eq!(runs(&shipped_plugin), Ok(()));
    assert_eq!(files(), 1);
    let _ = fs::remove_dir_all(&kept);
    let _ = fs::remove_dir_all(&shipped);
    let _ = fs::remove_dir_all(&built);
}

#[test]
fn a_bundled_plugin_is_let_in_unasked_and_stays() {
    let root = home("bundled");
    let bundle = root.join("bundle");
    let folder = bundle.join("test.bundled");
    fs::create_dir_all(&folder).unwrap();
    fs::write(
        folder.join(MANIFEST),
        format!(
            "id = \"test.bundled\"\nname = \"Bundled\"\nversion = \
             \"0.1.0\"\napi = \
             \"{API_VERSION}\"\n\n[[asks]]\ncapability = \
             \"net.fetch\"\ntarget = \"example.org\"\nreason = \"To \
             fetch\"\n"
        ),
    )
    .unwrap();
    fs::write(
        folder.join(WASM),
        component(&module("(i32.const 32)", ""), false),
    )
    .unwrap();
    let mut app = engine(&root);
    app.insert_resource(Bundle::at(&bundle));
    for _ in 0..3 {
        app.update();
    }
    let id = PluginId::try_from("test.bundled").unwrap();
    let world = app.world_mut();
    assert_eq!(world.query::<&Petition>().iter(world).count(), 0);
    let plugin = world
        .query_filtered::<Entity, With<Admitted>>()
        .single(world)
        .expect("it is let in");
    let record = world.resource::<Grants>().record(&id).cloned();
    let record = record.expect("it is recorded");
    assert!(record.bundled);
    assert_eq!(
        record.answer(Capability::NetFetch, "example.org"),
        Some(Answer::Allow)
    );

    world.trigger(Uninstall { entity: plugin });
    app.update();
    assert!(app.world().get_entity(plugin).is_ok());
    assert!(folder.join(WASM).is_file());
    assert!(app.world().resource::<Grants>().record(&id).is_some());
    let _ = fs::remove_dir_all(&root);
}

/// How a folder to add a plugin from is made.
#[derive(Clone, Copy, Debug)]
enum Made {
    Built,
    /// Its `plugin.wasm` a link to one elsewhere.
    Linked,
    /// Its `plugin.wasm` a named pipe.
    Special,
    Oversized,
    InHome,
    /// The source of a plugin, not built.
    Source,
    /// Asking for keys.
    Never,
    /// Asking for limits of its own.
    Limits,
}

#[test]
fn only_the_folder_of_a_built_plugin_is_added() {
    let root = home("adding");
    let outside = home("adding_from");
    let plugins = root.join(FOLDER);
    fs::create_dir_all(&plugins).unwrap();
    let tuning = WasmTuning {
        wasm_bytes: 64,
        ..WasmTuning::designed()
    };
    let into = plugins.join("test.adding");
    let mut cases = vec![
        (Made::Built, ""),
        (
            Made::Oversized,
            "plugin.wasm has 65 bytes, and the most is 64",
        ),
        (Made::InHome, "the folder is in the engine's home"),
        (Made::Source, "it has no plugin.wasm"),
        (Made::Never, "keys.read, and reading keys is never granted"),
        (Made::Limits, "only the person sets a plugin's limits"),
    ];
    if cfg!(unix) {
        cases.push((Made::Linked, "plugin.wasm is a link"));
        cases
            .push((Made::Special, "plugin.wasm is not a plain file"));
    }
    for (made, refusal) in cases {
        let folder = match made {
            Made::InHome => plugins.join("inside"),
            _ => outside.join(format!("{made:?}")),
        };
        fs::create_dir_all(&folder).unwrap();
        let asks = match made {
            Made::Never => {
                "[[asks]]\ncapability = \"keys.read\"\ntarget = \
                 \"all\"\nreason = \"To sign\"\n"
            }
            Made::Limits => "[limits]\nmemory = 65536\n",
            _ => "",
        };
        fs::write(
            folder.join(MANIFEST),
            format!(
                "id = \"test.adding\"\nname = \"Adding\"\nversion = \
                 \"0.1.0\"\napi = \"{API_VERSION}\"\n{asks}"
            ),
        )
        .unwrap();
        let wasm = folder.join(WASM);
        match made {
            Made::Linked => {
                fs::write(outside.join("elsewhere.wasm"), [0; 8])
                    .unwrap();
                #[cfg(unix)]
                std::os::unix::fs::symlink(
                    outside.join("elsewhere.wasm"),
                    &wasm,
                )
                .unwrap();
            }
            Made::Special => {
                let made = Command::new("mkfifo").arg(&wasm).status();
                assert!(made.unwrap().success());
            }
            Made::Oversized => fs::write(&wasm, [0; 65]).unwrap(),
            Made::Source => {
                fs::write(folder.join("Cargo.toml"), "").unwrap()
            }
            _ => fs::write(&wasm, [0; 64]).unwrap(),
        }
        let added =
            check(&folder, &root, &tuning).and_then(|checked| {
                checked.place(&plugins, &into, &tuning)
            });
        match added {
            Ok(manifest) => {
                assert!(refusal.is_empty(), "{made:?} was added");
                assert_eq!(manifest.id.as_str(), "test.adding");
                assert_eq!(
                    fs::read(into.join(WASM)).unwrap(),
                    [0; 64]
                );
                fs::remove_dir_all(&into).unwrap();
            }
            Err(why) => assert!(
                why.contains(refusal) && !refusal.is_empty(),
                "{made:?}: {why}"
            ),
        }
        // Nothing is left behind in the plugins folder.
        let left = fs::read_dir(&plugins)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .filter(|name| name != "inside")
            .collect::<Vec<_>>();
        assert!(left.is_empty(), "{made:?} left {left:?}");
    }
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&outside);
}

/// A home of its own for the test `name`, empty.
fn home(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "hamlet_engine_test_{name}_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    root
}

/// The engine without a window, on the home `root`.
fn engine(root: &Path) -> App {
    let mut app = App::new();
    app.insert_resource(Home::at(root)).add_plugins((
        TaskPoolPlugin::default(),
        TimePlugin,
        hamlet_engine_core::plugin,
        hamlet_engine_audio::plugin,
        hamlet_engine_plugins::plugin,
        hamlet_engine_grants::plugin,
        crate::plugin,
    ));
    app
}
