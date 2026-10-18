//! Compiled components kept in the plugin's folder, so that a restart
//! or a reload of a file that did not change does not compile again.

use std::hash::{Hash, Hasher};
use std::path::Path;

use bevy::prelude::*;
use sha2::{Digest, Sha256};
use wasmtime::Engine;
use wasmtime::component::Component as Compiled;

use crate::fault::Fault;

/// The folder of the cache, in the plugin's folder.
pub const FOLDER: &str = ".cache";

const EXTENSION: &str = "cwasm";

/// Where a compiled component came from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Origin {
    Cache,
    Compiled,
}

/// The component in `file`, from the cache beside it when an entry
/// for this file, engine and configuration is there and can be read,
/// and compiled otherwise. A compiled one is kept, and replaces the
/// entries of other files.
pub fn component(
    engine: &Engine,
    file: &Path,
) -> Result<(Compiled, Origin), Fault> {
    let bytes = std::fs::read(file).map_err(|e| {
        Fault::Load(format!("{}: {e}", file.display()))
    })?;
    let folder = file.parent().unwrap_or(Path::new("")).join(FOLDER);
    let name = format!("{}.{EXTENSION}", key(engine, &bytes));
    let entry = folder.join(&name);
    if entry.is_file() {
        // SAFETY: the entry was written by this function from a
        // component this engine compiled, and is checked against the
        // engine's version and configuration as it is read.
        match unsafe { Compiled::deserialize_file(engine, &entry) } {
            Ok(component) => return Ok((component, Origin::Cache)),
            Err(why) => {
                debug!("{}: ignored: {why:#}", entry.display());
                let _ = std::fs::remove_file(&entry);
            }
        }
    }
    let component =
        Compiled::new(engine, &bytes).map_err(Fault::load)?;
    if let Err(why) = keep(&folder, &name, &component) {
        warn!("{}: not cached: {why}", folder.display());
    }
    Ok((component, Origin::Compiled))
}

/// The name of the entry for `bytes` compiled by `engine`: a hash of
/// the file, then one of the engine's version and configuration.
fn key(engine: &Engine, bytes: &[u8]) -> String {
    let file = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let mut engine_hash = std::hash::DefaultHasher::new();
    engine
        .precompile_compatibility_hash()
        .hash(&mut engine_hash);
    format!("{file}-{:016x}", engine_hash.finish())
}

/// Writes `component` as the entry `name` of `folder`, and takes out
/// the entries of other files.
fn keep(
    folder: &Path,
    name: &str,
    component: &Compiled,
) -> Result<(), String> {
    let bytes = component.serialize().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
    if let Ok(entries) = std::fs::read_dir(folder) {
        for entry in entries.filter_map(|entry| entry.ok()) {
            if entry.file_name() != name {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    let part = folder.join(format!("{name}.part"));
    std::fs::write(&part, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&part, folder.join(name))
        .map_err(|e| e.to_string())
}
