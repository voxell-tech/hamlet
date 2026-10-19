//! The window's dependency boundary (S5, ADR 0004).

use std::path::Path;

const MANIFESTS: [&str; 6] = [
    "Cargo.toml",
    "crates/hamlet_client/Cargo.toml",
    "crates/hamlet_protocol/Cargo.toml",
    "crates/hamlet_realm/Cargo.toml",
    "crates/hamlet_ui/Cargo.toml",
    "crates/hamlet_desktop/Cargo.toml",
];

const RUNTIME: [&str; 1] = ["hamlet-server"];

fn runtime_dependencies(manifest: &str) -> Vec<String> {
    fn visit(table: &toml::Table, found: &mut Vec<String>) {
        for (key, value) in table {
            let Some(table) = value.as_table() else {
                continue;
            };
            match key.as_str() {
                "workspace" => continue,
                "dependencies" | "dev-dependencies"
                | "build-dependencies" => {
                    for (name, dependency) in table {
                        let package = dependency
                            .get("package")
                            .and_then(toml::Value::as_str)
                            .unwrap_or(name);
                        if RUNTIME.contains(&package) {
                            found.push(package.to_owned());
                        }
                    }
                }
                _ => visit(table, found),
            }
        }
    }
    let manifest = manifest
        .parse::<toml::Table>()
        .expect("valid Cargo manifest");
    let mut found = Vec::new();
    visit(&manifest, &mut found);
    found
}

#[test]
fn no_window_package_depends_on_the_runtime() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let violations = MANIFESTS
        .iter()
        .filter_map(|path| {
            let manifest =
                std::fs::read_to_string(root.join(path)).unwrap();
            let runtime = runtime_dependencies(&manifest);
            (!runtime.is_empty())
                .then(|| format!("{path}: {}", runtime.join(", ")))
        })
        .collect::<Vec<_>>();
    assert!(
        violations.is_empty(),
        "The window runs only the client:\n{}",
        violations.join("\n")
    );
}

#[test]
fn runtime_dependencies_are_caught_in_every_manifest_and_dependency_section()
 {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for path in MANIFESTS {
        let manifest =
            std::fs::read_to_string(root.join(path)).unwrap();
        for runtime in RUNTIME {
            for section in [
                "dependencies",
                "dev-dependencies",
                "build-dependencies",
                "target.'cfg(unix)'.dependencies",
            ] {
                let injected = format!(
                    "{manifest}\n[{section}.runtime_probe]\npackage \
                     = \"{runtime}\"\nversion = \"*\"\n"
                );
                assert!(
                    runtime_dependencies(&injected)
                        .iter()
                        .any(|name| name == runtime),
                    "{path}: {section} failed to catch {runtime}"
                );
            }
        }
    }
    assert!(
        runtime_dependencies(
            "[workspace.dependencies]\nhamlet-server = { path = \
             'crates/hamlet-server' }\n"
        )
        .is_empty()
    );
}
