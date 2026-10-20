//! The append-only JSONL records the rules are read from.

use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};
use chrono::TimeZone;
use hamlet_model::log;
use serde_json::Value;

/// Every line of the record at `path`, oldest first. Empty when it
/// cannot be read.
pub(crate) fn read(path: &Path) -> Vec<Value> {
    log::read_events(path).unwrap_or_default()
}

/// Adds `event` to the record at `path` as one line.
pub(crate) fn append(path: &Path, event: &Value) -> Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("opening {}", path.display()))?;
    // One write, so two writers at the same moment cannot mix their
    // lines.
    file.write_all(format!("{event}\n").as_bytes())
        .with_context(|| format!("writing {}", path.display()))
}

/// The text under `key` of a line, empty when it has none.
pub(crate) fn text(e: &Value, key: &str) -> String {
    e[key].as_str().unwrap_or_default().to_string()
}

/// A time of the record, as a chief reads it.
pub(crate) fn when(ts: u64) -> String {
    chrono::Local
        .timestamp_millis_opt(ts as i64)
        .single()
        .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default()
}
