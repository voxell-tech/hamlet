// This file was changed for Hamlet. See NOTICE in this crate.
use std::fs;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_dir =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let out_dir = PathBuf::from(std::env::var("OUT_DIR")?);

    // Each protocol's code goes under a folder of OUT_DIR named for
    // it, and its module file includes from that folder.
    for name in ["persist", "client_protocol", "inspector_protocol"] {
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
    }

    Ok(())
}
