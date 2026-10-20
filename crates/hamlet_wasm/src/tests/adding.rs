use std::fs;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use hamlet_agents_api::oversight::Choice as Answer;
use hamlet_desktop::file::{FileDialog, unseen};
use hamlet_sdk::capability::{AddAnswer, AddPlugin, Adding, Subject};
use hamlet_sdk::plugin::{PluginInfo, Status};

use super::permissions::{answer, petitions};
use super::tree::items;
use crate::add::Adder;
use crate::add::check::{self, Known, Make};
use crate::fixture;
use crate::fixture::home::{self, Scratch};
use crate::interface::VERSION;
use crate::limits::Limits;

fn manifest(id: &str, interface: &str) -> String {
    format!(
        "id = \"{id}\"\nname = \"Probe\"\nversion = \
         \"0.1.0\"\ninterface = \"{interface}\"\n"
    )
}

/// A folder outside the home holding `files`.
fn folder(
    outside: &Scratch,
    name: &str,
    files: &[(&str, &[u8])],
) -> PathBuf {
    let folder = outside.home().join(name);
    fs::create_dir_all(&folder).unwrap();
    for (file, bytes) in files {
        fs::write(folder.join(file), bytes).unwrap();
    }
    folder
}

fn read(
    home: &Path,
    chosen: &Path,
    known: &[Known],
) -> Result<check::Plan, String> {
    check::read(chosen, home, &Limits::default(), known, None)
}

#[test]
fn a_folder_is_a_built_plugin_the_source_of_one_or_refused_with_why()
{
    let (scratch, outside) =
        (Scratch::new("adding_home"), Scratch::new("adding_outside"));
    fs::create_dir_all(scratch.plugins()).unwrap();
    let probe = manifest("probe", VERSION);

    let built = folder(
        &outside,
        "built",
        &[
            ("manifest.toml", probe.as_bytes()),
            ("plugin.wasm", b"wasm"),
        ],
    );
    let plan = read(&scratch.home(), &built, &[]).unwrap();
    assert_eq!(plan.make, Make::Copy);
    let names = plan.files.iter().map(|(name, ..)| name.as_str());
    assert_eq!(
        names.collect::<Vec<_>>(),
        ["plugin.wasm", "manifest.toml"]
    );

    let cargo = b"[package]\nname = \"tide-table\"\n";
    let source = folder(
        &outside,
        "source",
        &[("manifest.toml", probe.as_bytes()), ("Cargo.toml", cargo)],
    );
    let plan = read(&scratch.home(), &source, &[]).unwrap();
    assert_eq!(plan.make, Make::Build("tide_table".into()));
    let installed = Known {
        id: "probe".into(),
        name: "Probe".into(),
        version: "0.0.9".into(),
        builtin: false,
        native: false,
        installed: true,
    };
    let plan = read(
        &scratch.home(),
        &source,
        std::slice::from_ref(&installed),
    )
    .unwrap();
    assert_eq!(plan.replaces, Some(("Probe 0.0.9".into(), true)));

    let refused = |name: &str,
                   files: &[(&str, &[u8])],
                   known: &[Known]| {
        read(&scratch.home(), &folder(&outside, name, files), known)
            .unwrap_err()
    };
    let old = manifest("probe", "0.0.1");
    let host = manifest("host", VERSION);
    let workspace: &[u8] = b"[workspace]\nmembers = [\"a\"]\n";
    assert!(refused("bare", &[], &[]).contains("no manifest.toml"));
    assert!(
        refused("old", &[("manifest.toml", old.as_bytes())], &[])
            .contains("built for interface 0.0.1")
    );
    assert!(
        refused("host", &[("manifest.toml", host.as_bytes())], &[])
            .contains("reserved")
    );
    assert!(
        refused("empty", &[("manifest.toml", probe.as_bytes())], &[])
            .contains("neither plugin.wasm nor Cargo.toml")
    );
    assert!(
        refused(
            "many",
            &[
                ("manifest.toml", probe.as_bytes()),
                ("Cargo.toml", workspace)
            ],
            &[]
        )
        .contains("names no package")
    );
    let native = Known {
        native: true,
        ..installed
    };
    assert!(
        refused(
            "clash",
            &[
                ("manifest.toml", probe.as_bytes()),
                ("plugin.wasm", b"w")
            ],
            &[native]
        )
        .contains("uninstall it first")
    );
    let inside = scratch.home().join("inside");
    fs::create_dir_all(&inside).unwrap();
    assert!(
        read(&scratch.home(), &inside, &[])
            .unwrap_err()
            .contains("Hamlet's home")
    );

    #[cfg(unix)]
    {
        let linked = folder(
            &outside,
            "linked",
            &[("manifest.toml", probe.as_bytes())],
        );
        std::os::unix::fs::symlink(
            built.join("plugin.wasm"),
            linked.join("plugin.wasm"),
        )
        .unwrap();
        let plan = read(&scratch.home(), &linked, &[]).unwrap();
        let placed = crate::add::place::place(
            &plan,
            &scratch.plugins(),
            &Limits::default(),
            None,
        );
        assert_eq!(placed, Err("plugin.wasm is a link".into()));
        assert!(!scratch.plugins().join("probe").exists());
    }
}

#[test]
fn a_built_folder_the_chief_picks_is_copied_in_and_asked_about_at_once()
 {
    let (scratch, outside) = (
        Scratch::new("adding_app"),
        Scratch::new("adding_app_outside"),
    );
    let component = fixture::component(&fixture::drawing(
        &items("a"),
        &items("b"),
    ));
    let built = folder(
        &outside,
        "probe-build",
        &[
            ("manifest.toml", manifest("probe", VERSION).as_bytes()),
            ("plugin.wasm", &component),
        ],
    );
    let mut app = home::app(&scratch, &[]);
    app.insert_resource(FileDialog(unseen));
    app.update();

    app.world_mut().trigger(AddPlugin);
    app.update();
    assert_eq!(*app.world().resource::<Adding>(), Adding::Choosing);
    app.world().resource::<Adder>().choose(Some(built.clone()));
    app.update();
    app.update();

    assert!(matches!(
        app.world().resource::<Adding>(),
        Adding::Added(_)
    ));
    let copied = scratch.plugins().join("probe");
    assert_eq!(
        fs::read(copied.join("plugin.wasm")).unwrap(),
        component
    );
    assert!(!scratch.plugins().join(".adding").exists());
    let asked = petitions(&mut app);
    assert!(matches!(
        &asked[..],
        [(_, Subject::Install { plugin })] if plugin == "probe"
    ));
    answer(&mut app, asked[0].0, Answer::Allow);
    let mut plugins =
        app.world_mut().query::<(&PluginInfo, &Status)>();
    assert_eq!(
        *plugins.single(app.world()).unwrap().1,
        Status::Running
    );

    // The same again takes its place as an update, once the chief
    // says so.
    app.world_mut().trigger(AddPlugin);
    app.update();
    app.world().resource::<Adder>().choose(Some(built));
    app.update();
    assert!(matches!(
        app.world().resource::<Adding>(),
        Adding::Asking(question) if question.replaces.is_some()
    ));
    app.world_mut().trigger(AddAnswer { go: false });
    assert_eq!(*app.world().resource::<Adding>(), Adding::Idle);
}

#[test]
#[ignore = "builds a guest"]
fn the_source_of_a_guest_is_built_and_added() {
    let scratch = Scratch::new("adding_source");
    let roster = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../guests/roster");
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let mut app = home::app(&scratch, &[]);
    app.insert_resource(FileDialog(unseen)).insert_resource(
        hamlet_base::Tokio(runtime.handle().clone()),
    );
    app.update();
    app.world_mut().trigger(AddPlugin);
    app.update();
    app.world().resource::<Adder>().choose(Some(roster));
    let wait = |app: &mut App, done: fn(&Adding) -> bool| {
        for _ in 0..30000 {
            app.update();
            if done(app.world().resource::<Adding>()) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        panic!("{:?}", app.world().resource::<Adding>());
    };
    wait(
        &mut app,
        |adding| matches!(adding, Adding::Asking(q) if q.build),
    );
    app.world_mut().trigger(AddAnswer { go: true });
    wait(&mut app, |adding| adding.free());
    assert!(
        matches!(app.world().resource::<Adding>(), Adding::Added(_)),
        "{:?}",
        app.world().resource::<Adding>()
    );
    app.update();
    assert!(petitions(&mut app).iter().any(|(_, subject)| matches!(
        subject,
        Subject::Install { plugin } if plugin == "roster"
    )));
}
