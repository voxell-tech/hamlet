//! The layers the crates of the workspace stand on, read from cargo's
//! metadata, where a dependency is named by its package whatever it
//! is renamed to.

use std::path::Path;
use std::process::Command;

use serde_json::Value;

/// The crates of the engine: each stands on no layer, nor on a crate
/// that does, its tests neither.
const ENGINES: [&str; 7] = [
    "hamlet_sdk",
    "hamlet_base",
    "hamlet_wasm",
    "hamlet_native",
    "hamlet_desktop",
    "hamlet_ui",
    "hamlet_openui",
];

/// The engines of the layers and the crates under them.
const LAYERS: [&str; 6] = [
    "hamlet_core",
    "hamlet_rules",
    "hamlet_realm",
    "hamlet_model",
    "hamlet_home",
    "hamlet_sandbox",
];

/// The frame of the window, a plugin as the others are.
const FRAME: &str = "hamlet_ui";

/// The list of the built-in plugins, which names every one.
const LIST: &str = "hamlet_plugins";

/// The layers under `hamlet_agents_api`.
const AGENTS_API: &[&str] = &[
    "hamlet_core",
    "hamlet_home",
    "hamlet_model",
    "hamlet_rules",
    "hamlet_sandbox",
];

/// A dependency that leads to a layer and stands for now: the member,
/// the package it depends on, the layers the walk from an engine or
/// an API crate lets through it, whether the dependency must be only
/// for the member's tests, and why.
type Exception = (
    &'static str,
    &'static str,
    &'static [&'static str],
    bool,
    &'static str,
);

/// The dependencies that lead to a layer and stand for now.
const EXCEPTIONS: &[Exception] = &[
    (
        "hamlet_base",
        "hamlet_home",
        &["hamlet_home"],
        false,
        "`HomeDir` holds hamlet_home's `Home`; a layout of its own \
         in hamlet_base would put Bevy under the rules",
    ),
    (
        "hamlet_wasm",
        "hamlet_agents_api",
        AGENTS_API,
        false,
        "the agents' API re-exports the core's types, and taking it \
         off the core is a design of its own",
    ),
    (
        "hamlet_ui",
        "hamlet_agents_api",
        AGENTS_API,
        false,
        "the frame adds the questions' card and the chat's read \
         models, which the agents' API keeps from the core",
    ),
    (
        "hamlet_ui",
        "hamlet_chat_api",
        AGENTS_API,
        false,
        "the chat's API stands on the agents' API",
    ),
    (
        "hamlet_wasm",
        "hamlet_home",
        &["hamlet_home"],
        true,
        "its tests make a `HomeDir`, which holds hamlet_home's \
         `Home`",
    ),
    (
        "hamlet_native",
        "hamlet_home",
        &["hamlet_home"],
        true,
        "its tests make a `HomeDir`, which holds hamlet_home's \
         `Home`",
    ),
    (
        "hamlet_ui",
        "hamlet_home",
        &["hamlet_home"],
        true,
        "its tests make a `HomeDir`, which holds hamlet_home's \
         `Home`",
    ),
];

/// The API crates that own their words: each stands on no layer, nor
/// on a crate that does, its tests neither.
const OWNERS: [&str; 2] = ["hamlet_realm_api", "hamlet_limits_api"];

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
    } else if !plugin || dependency.dev {
        None
    } else if to == "hamlet_core" && !layers {
        Some("a plugin reads the core through `hamlet_agents_api`")
    } else if to == "hamlet_rules" && !layers {
        Some("a plugin reads the rules through `hamlet_agents_api`")
    } else if matches!(to, "hamlet_model" | "hamlet_sandbox")
        && !layers
    {
        Some(
            "a plugin reads the model and the sandbox through \
             `hamlet_agents_api`",
        )
    } else if to == "hamlet_realm" && member != "plugin_realm" {
        Some("only `plugin_realm` depends on the realm's engine")
    } else {
        None
    }
}

/// The dependencies of the member `name` among `members`.
fn dependencies<'a>(
    name: &str,
    members: &'a [(String, String, Vec<Dependency>)],
) -> &'a [Dependency] {
    members
        .iter()
        .find(|(member, ..)| member == name)
        .map(|(.., dependencies)| dependencies.as_slice())
        .unwrap_or_default()
}

/// The layers `member` is built on through its dependencies `first`,
/// which may be its tests': those and the ones under them, but those
/// an exception on the way lets through.
fn layers_under<'a>(
    member: &'a str,
    first: impl IntoIterator<Item = &'a Dependency>,
    members: &'a [(String, String, Vec<Dependency>)],
    exceptions: &[Exception],
) -> Vec<&'a str> {
    // Each crate reached, with the layers let through on the way.
    let mut seen = Vec::<(&str, Vec<&str>)>::new();
    let mut found = Vec::new();
    let mut next = first
        .into_iter()
        .map(|dependency| (member, dependency, Vec::new()))
        .collect::<Vec<_>>();
    while let Some((from, dependency, mut through)) = next.pop() {
        let to = dependency.package.as_str();
        for (.., layers, _, _) in
            exceptions.iter().filter(|(m, d, _, tests, _)| {
                (*m, *d) == (from, to) && (!tests || dependency.dev)
            })
        {
            through.extend(layers.iter().copied());
        }
        through.sort_unstable();
        through.dedup();
        if seen.contains(&(to, through.clone())) {
            continue;
        }
        seen.push((to, through.clone()));
        if LAYERS.contains(&to)
            && !through.contains(&to)
            && !found.contains(&to)
        {
            found.push(to);
        }
        next.extend(
            dependencies(to, members)
                .iter()
                .filter(|d| !d.dev)
                .map(|dependency| (to, dependency, through.clone())),
        );
    }
    found
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
                found.push((
                    member.as_str(),
                    &*dependency.package,
                    rule,
                ));
            }
        }
    }
    for walker in ENGINES.into_iter().chain(OWNERS) {
        assert!(members.iter().any(|(name, ..)| name == walker));
        let rule = match OWNERS.contains(&walker) {
            true => {
                "an API crate that owns its words stands on no layer"
            }
            false => "an engine crate stands on no layer",
        };
        let first = dependencies(walker, &members);
        for layer in layers_under(walker, first, &members, EXCEPTIONS)
        {
            found.push((walker, layer, rule));
        }
    }
    // An exception names a dependency that is there, of the kind it
    // says, and each layer it lets through is behind it.
    for &(member, dependency, layers, tests, _) in EXCEPTIONS {
        let edges = dependencies(member, &members)
            .iter()
            .filter(|d| d.package == dependency)
            .collect::<Vec<_>>();
        if edges.is_empty() {
            found.push((
                member,
                dependency,
                "an exception that is gone",
            ));
        } else if tests && edges.iter().any(|d| !d.dev) {
            found.push((
                member,
                dependency,
                "an exception for tests on a dependency that is not \
                 only theirs",
            ));
        }
        let behind = layers_under(member, edges, &members, &[]);
        for layer in layers.iter().filter(|l| !behind.contains(l)) {
            found.push((
                dependency,
                layer,
                "an exception lets through a layer that is not \
                 behind it",
            ));
        }
    }
    assert!(found.is_empty(), "{found:#?}");
}

#[test]
fn an_exception_lets_through_only_the_layers_it_names() {
    let member = |name: &str, on: &[(&str, bool)]| {
        let dependencies = on
            .iter()
            .map(|(package, dev)| Dependency {
                package: package.to_string(),
                dev: *dev,
            })
            .collect();
        (name.to_string(), String::new(), dependencies)
    };
    let members = [
        member("engine", &[("api", false), ("hamlet_home", false)]),
        member(
            "api",
            &[("hamlet_core", false), ("hamlet_model", false)],
        ),
    ];
    let exceptions: &[Exception] = &[
        ("engine", "api", &["hamlet_core"], false, ""),
        ("engine", "hamlet_home", &["hamlet_home"], true, ""),
    ];
    let first = dependencies("engine", &members);

    let mut found =
        layers_under("engine", first, &members, exceptions);
    found.sort_unstable();
    assert_eq!(found, ["hamlet_home", "hamlet_model"]);
}
