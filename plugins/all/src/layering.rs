//! The layers the plugin crates stand on, read from their manifests.

use std::path::Path;

use toml::Table;

const TABLES: [&str; 3] =
    ["dependencies", "dev-dependencies", "build-dependencies"];

/// The rule `plugin` breaks by depending on `dependency`.
fn broken(plugin: &str, dependency: &str) -> Option<&'static str> {
    if dependency.starts_with("plugin_") {
        Some("a plugin depends on no other plugin")
    } else if dependency == "hamlet_ui" {
        Some("a plugin never depends on the window's frame")
    } else if dependency == "hamlet_realm" && plugin != "plugin_realm"
    {
        Some("only `plugin_realm` depends on the realm's engine")
    } else if dependency == "hamlet_core"
        && !matches!(plugin, "plugin_agents" | "plugin_hamlets")
    {
        Some("a plugin reads the core through `hamlet_agents_api`")
    } else {
        None
    }
}

/// The names of every crate `manifest` depends on, on any target.
fn dependencies(manifest: &Table) -> Vec<String> {
    let targets = manifest
        .get("target")
        .and_then(|targets| targets.as_table())
        .into_iter()
        .flat_map(|targets| targets.values())
        .filter_map(|target| target.as_table());
    std::iter::once(manifest)
        .chain(targets)
        .flat_map(|table| {
            TABLES
                .iter()
                .filter_map(|kind| table.get(*kind)?.as_table())
        })
        .flat_map(|table| table.keys().cloned())
        .collect()
}

#[test]
fn the_plugin_crates_keep_to_their_layers() {
    let plugins = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut found = Vec::new();
    for folder in std::fs::read_dir(&plugins).unwrap() {
        let path = folder.unwrap().path().join("Cargo.toml");
        let manifest = std::fs::read_to_string(&path)
            .unwrap()
            .parse::<Table>()
            .unwrap();
        let name = manifest["package"]["name"].as_str().unwrap();
        if !name.starts_with("plugin_") {
            continue;
        }
        for dependency in dependencies(&manifest) {
            if let Some(rule) = broken(name, &dependency) {
                found.push((name.to_string(), dependency, rule));
            }
        }
    }
    assert!(found.is_empty(), "{found:#?}");
}
