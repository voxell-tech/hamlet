//! The window's parts reach the other Hamlet crates only through
//! `hamlet_core`, as a plugin does.

use std::fs;
use std::path::Path;

use toml::Table;

/// The crate that puts the parts together, which may use any crate.
const WINDOW: &str = "hamlet_app";

/// The Hamlet crates a part may use besides `hamlet_core`, each a
/// helper of that part alone.
const OWN_HELPERS: &[(&str, &str)] =
    &[("hamlet_realm", "hamlet_vfx")];

/// The Hamlet crates a manifest's `section` names.
fn hamlet_crates(manifest: &Table, section: &str) -> Vec<String> {
    manifest
        .get(section)
        .and_then(|deps| deps.as_table())
        .map(|deps| {
            deps.keys()
                .filter(|name| name.starts_with("hamlet_"))
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn a_part_uses_no_hamlet_crate_but_core() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut wrong = Vec::new();
    for folder in fs::read_dir(crates).unwrap() {
        let file = folder.unwrap().path().join("Cargo.toml");
        let Ok(text) = fs::read_to_string(&file) else {
            continue;
        };
        let manifest = text.parse::<Table>().unwrap();
        let name = manifest["package"]["name"].as_str().unwrap();
        let uses = hamlet_crates(&manifest, "dependencies");
        // A part is a crate built on core, other than the window.
        if name == WINDOW
            || !uses.iter().any(|used| used == "hamlet_core")
        {
            continue;
        }
        let tests = hamlet_crates(&manifest, "dev-dependencies");
        for used in uses.iter().chain(&tests) {
            let allowed = used == "hamlet_core"
                || OWN_HELPERS.contains(&(name, used.as_str()));
            if !allowed {
                wrong.push(format!("{name} uses {used}"));
            }
        }
    }
    wrong.sort();
    assert!(
        wrong.is_empty(),
        "a part reaches other Hamlet crates through hamlet_core: {}",
        wrong.join(", "),
    );
}
