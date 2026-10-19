//! The layers the crates of the workspace stand on, read from cargo's
//! metadata, where a dependency is named by its package whatever it
//! is renamed to.

use std::path::Path;
use std::process::Command;

use serde_json::Value;

/// The crates of the engine, which stand on no layer.
const ENGINES: [&str; 7] = [
    "hamlet_sdk",
    "hamlet_base",
    "hamlet_wasm",
    "hamlet_native",
    "hamlet_desktop",
    "hamlet_ui",
    "hamlet_openui",
];

/// The engines of the layers.
const LAYERS: [&str; 3] =
    ["hamlet_core", "hamlet_rules", "hamlet_realm"];

/// The frame of the window, a plugin as the others are.
const FRAME: &str = "hamlet_ui";

/// The list of the built-in plugins, which names every one.
const LIST: &str = "hamlet_plugins";

/// An engine's dependency on a layer that stands for now, and why.
const EXCEPTIONS: &[(&str, &str, &str)] = &[];

/// One dependency of a member: the package it names, and whether it
/// is only for the member's tests.
struct Dependency {
    package: String,
    dev: bool,
}

/// The rule `member` breaks by depending on `dependency`. A plugin's
/// tests may stand up a layer it reads through an API crate, but may
/// not name another plugin.
fn broken(
    member: &str,
    dependency: &Dependency,
    plugins: &[String],
) -> Option<&'static str> {
    let plugin = plugins.iter().any(|name| name == member);
    let to = dependency.package.as_str();
    let layers = matches!(member, "plugin_agents" | "plugin_hamlets");
    if plugin && plugins.iter().any(|name| name == to) {
        Some("a plugin depends on no other plugin, nor on the frame")
    } else if member.ends_with("_api")
        && plugins.iter().any(|n| n == to)
    {
        Some("an API crate depends on no plugin")
    } else if ENGINES.contains(&member)
        && LAYERS.contains(&to)
        && !EXCEPTIONS
            .iter()
            .any(|(m, d, _)| *m == member && *d == to)
    {
        Some("an engine crate stands on no layer")
    } else if !plugin || dependency.dev {
        None
    } else if to == "hamlet_core" && !layers {
        Some("a plugin reads the core through `hamlet_agents_api`")
    } else if to == "hamlet_rules" && !layers {
        Some("a plugin reads the rules through `hamlet_agents_api`")
    } else if to == "hamlet_realm" && member != "plugin_realm" {
        Some("only `plugin_realm` depends on the realm's engine")
    } else {
        None
    }
}

/// Each member of the workspace with its folder and its dependencies,
/// as `cargo metadata` gives them.
fn members() -> Vec<(String, String, Vec<Dependency>)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let cargo = std::env::var_os("CARGO").unwrap_or("cargo".into());
    let out = Command::new(cargo)
        .args(["metadata", "--offline", "--no-deps"])
        .args(["--format-version", "1", "--manifest-path"])
        .arg(root.join("Cargo.toml"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let metadata =
        serde_json::from_slice::<Value>(&out.stdout).unwrap();
    let text = |value: &Value| {
        value.as_str().unwrap_or_default().to_string()
    };
    metadata["packages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|package| {
            let dependencies = package["dependencies"]
                .as_array()
                .unwrap()
                .iter()
                .map(|dependency| Dependency {
                    package: text(&dependency["name"]),
                    dev: dependency["kind"] == "dev",
                })
                .collect();
            let path = text(&package["manifest_path"]);
            (text(&package["name"]), path, dependencies)
        })
        .collect()
}

#[test]
fn the_crates_keep_to_their_layers() {
    let members = members();
    let plugins = members
        .iter()
        .filter(|(name, path, _)| {
            let folder = Path::new(path).parent().unwrap();
            let under = folder.parent().unwrap().ends_with("plugins");
            (under && name != LIST) || name == FRAME
        })
        .map(|(name, ..)| name.clone())
        .collect::<Vec<_>>();
    for plugin in
        ["plugin_agents", "plugin_chat", "plugin_realm", FRAME]
    {
        assert!(
            plugins.iter().any(|name| name == plugin),
            "{plugin}"
        );
    }
    let mut found = Vec::new();
    for (member, _, dependencies) in &members {
        for dependency in dependencies {
            if let Some(rule) = broken(member, dependency, &plugins) {
                found.push((member, &dependency.package, rule));
            }
        }
    }
    assert!(found.is_empty(), "{found:#?}");
}
