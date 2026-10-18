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
