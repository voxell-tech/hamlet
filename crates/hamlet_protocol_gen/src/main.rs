//! Regeneration of the checked-in protocol v0 types.

use std::fs;
use std::path::Path;

fn main() -> Result<(), String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let schema = root.join("server/protocol.schema.json");
    let text = fs::read_to_string(&schema)
        .map_err(|error| format!("{}: {error}", schema.display()))?;
    let schema = serde_json::from_str(&text)
        .map_err(|error| error.to_string())?;
    let source = hamlet_protocol_gen::generate(&schema)?;
    let target = root.join("crates/hamlet_protocol/src/v0.rs");
    fs::write(&target, source)
        .map_err(|error| format!("{}: {error}", target.display()))
}
