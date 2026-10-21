//! Regeneration of the checked-in protocol v0 and harness link types.

use std::fs;
use std::path::Path;

/// Each schema with the Rust file generated from it.
const TARGETS: &[(&str, &str)] = &[(
    "harnesses/pi/link.schema.json",
    "crates/hamlet_server/src/harness/runtime.rs",
)];

fn main() -> Result<(), String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for (schema, target) in TARGETS {
        let schema = root.join(schema);
        let text = fs::read_to_string(&schema).map_err(|error| {
            format!("{}: {error}", schema.display())
        })?;
        let schema = serde_json::from_str(&text)
            .map_err(|error| error.to_string())?;
        let source = hamlet_protocol_gen::generate(&schema)?;
        let target = root.join(target);
        fs::write(&target, source).map_err(|error| {
            format!("{}: {error}", target.display())
        })?;
    }
    Ok(())
}
