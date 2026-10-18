use std::path::PathBuf;

use hamlet_wasm::fixture::{self, Item, Widget};
use hamlet_wasm::interface::VERSION;

use crate::script::Script;
use crate::{Harness, run};

/// A plugin folder whose panel `main` has a button, and a label that
/// reads "before" until the button is pressed.
fn folder(name: &str) -> PathBuf {
    folder_of(name, "probe")
}

/// The same, for the plugin `id`.
fn folder_of(name: &str, id: &str) -> PathBuf {
    let folder = std::env::temp_dir().join(format!(
        "hamlet_harness_{name}_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    let tree = |count| {
        vec![
            Item(None, Widget::Column, 2),
            Item(Some("go"), Widget::Button("Go"), 0),
            Item(Some("count"), Widget::Label(count), 0),
        ]
    };
    let module = fixture::drawing(&tree("before"), &tree("after"));
    std::fs::write(
        folder.join("plugin.wasm"),
        fixture::component(&module),
    )
    .unwrap();
    std::fs::write(
        folder.join("manifest.toml"),
        format!(
            "id = \"{id}\"\nname = \"Probe\"\nversion = \
             \"0.1.0\"\ninterface = \"{VERSION}\"\n"
        ),
    )
    .unwrap();
    folder
}

#[test]
fn a_click_changes_the_tree_and_a_render_writes_a_picture() {
    let plugin = folder("click");
    let mut harness = Harness::open(&plugin).unwrap();
    assert!(
        harness
            .outline("main")
            .contains(&"  label \"before\" #count".into())
    );

    harness.click("main", "go").unwrap();
    assert!(
        harness
            .outline("main")
            .contains(&"  label \"after\" #count".into())
    );

    let picture = plugin.join("main.png");
    harness.render("main", &picture).unwrap();
    let bytes = std::fs::read(&picture).unwrap();
    assert!(bytes.starts_with(b"\x89PNG"), "{} bytes", bytes.len());
    let _ = std::fs::remove_dir_all(&plugin);
}

#[test]
fn a_restart_brings_the_plugin_up_again_on_the_same_home() {
    let plugin = folder("restart");
    let mut harness = Harness::open(&plugin).unwrap();
    harness.click("main", "go").unwrap();
    let shown = |harness: &Harness, text: &str| {
        harness
            .outline("main")
            .iter()
            .any(|line| line.contains(text))
    };
    assert!(shown(&harness, "after"));

    harness.restart();

    assert!(shown(&harness, "before"));
    assert_eq!(harness.panels(), ["main"]);
    let _ = std::fs::remove_dir_all(&plugin);
}

#[test]
fn a_script_reports_the_step_that_failed_and_stops() {
    let plugin = folder("script");
    let mut harness = Harness::open(&plugin).unwrap();
    let script = Script::parse(
        "[[step]]\ntick = 1\n\n[[step]]\nclick = { panel = \
         \"main\", node = \"nope\" }\n\n[[step]]\ntick = 1\n",
    )
    .unwrap();

    let report = run::run(&mut harness, &script, &plugin);

    assert_eq!(report.failed, Some(2));
    let last = report.lines.last().unwrap();
    assert!(
        last.starts_with("FAILED step 2: panel `main` has no node")
    );
    assert!(
        !report.lines.iter().any(|line| line.starts_with("step 3"))
    );
    let _ = std::fs::remove_dir_all(&plugin);
}

#[test]
fn several_plugins_run_in_one_home_and_the_one_in_front_takes_the_clicks()
 {
    let (first, second) =
        (folder_of("many_a", "alpha"), folder_of("many_b", "beta"));
    let mut harness = Harness::open_all(&[&first, &second]).unwrap();
    assert_eq!(harness.ids(), ["alpha", "beta"]);
    let shows = |harness: &Harness, text: &str| {
        harness
            .outline("main")
            .contains(&format!("  label \"{text}\" #count"))
    };

    harness.select("beta").unwrap();
    harness.click("main", "go").unwrap();
    assert!(shows(&harness, "after"));
    harness.select("alpha").unwrap();
    assert!(shows(&harness, "before"));
    assert!(harness.select("gamma").is_err());
    let _ = std::fs::remove_dir_all(&first);
    let _ = std::fs::remove_dir_all(&second);
}

#[test]
fn a_menu_line_and_a_chord_run_a_command_once_the_chief_allows_them()
{
    let folder = std::env::temp_dir()
        .join(format!("hamlet_harness_menu_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join("plugin.wasm"),
        fixture::component(&fixture::contributing()),
    )
    .unwrap();
    std::fs::write(
        folder.join("manifest.toml"),
        format!(
            "id = \"far\"\nname = \"Far\"\nversion = \
             \"0.1.0\"\ninterface = \
             \"{VERSION}\"\n[exports]\ncommands = [\"go\"]\n{}",
            [
                "ui.menu",
                "ui.keys",
                "ui.context",
                "ui.inspector",
                "ui.settings"
            ]
            .map(|ask| format!(
                "[[asks]]\ncapability = \"{ask}\"\nreason = \"a \
                 test\"\n"
            ))
            .concat()
        ),
    )
    .unwrap();
    let mut harness = Harness::open(&folder).unwrap();
    let ran = |harness: &Harness| {
        harness
            .logs()
            .iter()
            .filter(|line| line.contains("hello world"))
            .count()
    };

    assert!(harness.run_menu("Go").is_err());
    harness.press("Cmd+G").unwrap();
    assert_eq!(ran(&harness), 0);

    harness.grant("ui.menu").unwrap();
    harness.grant("ui.keys").unwrap();
    harness.run_menu("Go").unwrap();
    assert_eq!(ran(&harness), 1);
    harness.press("Cmd+G").unwrap();
    assert_eq!(ran(&harness), 2);
    assert!(harness.run_menu("Nope").is_err());
    assert!(harness.run_command("far/nope").is_err());

    // A context item, a tab and a page show once they are allowed.
    assert!(harness.run_context("agent:Juniper", "Go").is_err());
    assert!(
        harness.sections().is_empty() && harness.pages().is_empty()
    );
    for ask in ["ui.context", "ui.inspector", "ui.settings"] {
        harness.grant(ask).unwrap();
    }
    harness.run_context("agent:Juniper", "Go").unwrap();
    assert_eq!(ran(&harness), 3);
    assert!(harness.run_context("house:Juniper", "Go").is_err());
    assert!(harness.run_context("juniper", "Go").is_err());
    assert_eq!(harness.sections(), ["Go"]);
    assert_eq!(harness.pages(), ["Go"]);
    let _ = std::fs::remove_dir_all(&folder);
}
