// This file was changed for Hamlet. See NOTICE in this crate.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_dir =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let out_dir = PathBuf::from(std::env::var("OUT_DIR")?);
    let repo_root = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .ok_or("Failed to find repository root")?;

    // Each protocol's code goes under a folder of OUT_DIR named for
    // it, and its module file includes from that folder.
    for (name, typescript_name) in [
        ("persist", None),
        ("client_protocol", Some("client-protocol")),
        ("inspector_protocol", Some("inspector")),
    ] {
        let schema_dir = manifest_dir.join("schemas").join(name);
        let protocol_out = out_dir.join(name);
        fs::create_dir_all(&protocol_out)?;
        let cfg = vbare_compiler::Config::default();
        vbare_compiler::process_schemas_to_dir(
            &schema_dir,
            &protocol_out,
            &cfg,
        )?;
        let imports = protocol_out.join("combined_imports.rs");
        let content = fs::read_to_string(&imports)?.replace(
            "env!(\"OUT_DIR\"), \"/",
            &format!("env!(\"OUT_DIR\"), \"/{name}/"),
        );
        fs::write(&imports, content)?;

        if let Some(typescript_name) = typescript_name {
            typescript::generate_versions(
                repo_root,
                &schema_dir,
                typescript_name,
            );
        }
    }

    Ok(())
}

mod typescript {
    use super::*;

    pub fn generate_versions(
        repo_root: &Path,
        schema_dir: &Path,
        protocol_name: &str,
    ) {
        let cli_js_path = repo_root
            .join("node_modules/@bare-ts/tools/dist/bin/cli.js");
        if !cli_js_path.exists() {
            println!(
                "cargo:warning=TypeScript codec generation skipped: \
                 cli.js not found at {}. Run `pnpm install` to \
                 install.",
                cli_js_path.display()
            );
            return;
        }

        let output_dir = repo_root
            .join("typescript")
            .join("packages")
            .join("hamlet_actor")
            .join("src")
            .join("common")
            .join("bare")
            .join("generated")
            .join(protocol_name);

        let _ = fs::remove_dir_all(&output_dir);
        fs::create_dir_all(&output_dir).expect(
            "Failed to create generated TypeScript codec directory",
        );

        for schema_path in schema_paths(schema_dir) {
            let version = schema_path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .expect("schema has valid UTF-8 file stem");
            let output_path =
                output_dir.join(format!("{version}.ts"));

            let output = Command::new(&cli_js_path)
                .arg("compile")
                .arg("--generator")
                .arg("ts")
                .arg(&schema_path)
                .arg("-o")
                .arg(&output_path)
                .output()
                .expect(
                    "Failed to execute bare compiler for TypeScript",
                );

            if !output.status.success() {
                panic!(
                    "BARE TypeScript generation failed for {}: {}",
                    schema_path.display(),
                    String::from_utf8_lossy(&output.stderr),
                );
            }

            post_process_generated_ts(&output_path);
        }
    }

    fn schema_paths(schema_dir: &Path) -> Vec<PathBuf> {
        let mut paths = fs::read_dir(schema_dir)
            .expect("Failed to read schema directory")
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension().and_then(|ext| ext.to_str())
                    == Some("bare")
            })
            .collect::<Vec<_>>();
        paths.sort();
        paths
    }

    const POST_PROCESS_MARKER: &str =
        "// @generated - post-processed by build.rs\n";

    fn post_process_generated_ts(path: &Path) {
        let content = fs::read_to_string(path)
            .expect("Failed to read generated TypeScript file");

        if content.starts_with(POST_PROCESS_MARKER) {
            return;
        }

        let content =
            content.replace("@bare-ts/lib", "@hamlet/bare-ts");
        let content =
            content.replace("import assert from \"assert\"", "");
        let content =
            content.replace("import assert from \"node:assert\"", "");

        let assert_function = r#"
function assert(condition: boolean, message?: string): asserts condition {
    if (!condition) throw new Error(message ?? "Assertion failed")
}
"#;
        let content = format!(
            "{}{}\n{}",
            POST_PROCESS_MARKER, content, assert_function
        );

        assert!(
            !content.contains("@bare-ts/lib"),
            "Failed to replace @bare-ts/lib import"
        );
        assert!(
            !content.contains("import assert from"),
            "Failed to remove Node.js assert import"
        );

        fs::write(path, content)
            .expect("Failed to write post-processed TypeScript file");
    }
}
