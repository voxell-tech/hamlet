//! Compiled components kept in the home beside the install folder,
//! never in a plugin's own folder, so that a restart or a reload of a
//! file that did not change does not compile again.

use std::hash::{Hash, Hasher};
use std::io::Read;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use sha2::{Digest, Sha256};
use wasmtime::Engine;
use wasmtime::component::Component as Compiled;

use crate::fault::Fault;

/// The folder of the caches in the home, one folder for each plugin.
pub const FOLDER: &str = "plugins-cache";

/// What a plugin's folder may not carry: a cache that came with it.
pub const SHIPPED: &str = ".cache";

const EXTENSION: &str = "cwasm";

/// The cache of the plugin whose component is `file`, a file in
/// `<home>/plugins/<id>`.
fn folder_of(file: &Path) -> Option<PathBuf> {
    let plugin = file.parent()?;
    let home = plugin.parent()?.parent()?;
    Some(home.join(FOLDER).join(plugin.file_name()?))
}

/// Deletes the cache of the plugin whose install `folder` is
/// `<home>/plugins/<id>`.
pub fn drop_cache(folder: &Path) {
    if let Some(cache) = folder_of(&folder.join("x")) {
        let _ = std::fs::remove_dir_all(cache);
    }
}

/// Where a compiled component came from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Origin {
    Cache,
    Compiled,
}

/// Deletes the cache a plugin's `folder` came with.
pub fn drop_shipped(folder: &Path) {
    let shipped = folder.join(SHIPPED);
    let Ok(meta) = std::fs::symlink_metadata(&shipped) else {
        return;
    };
    let _ = match meta.is_dir() {
        true => std::fs::remove_dir_all(&shipped),
        false => std::fs::remove_file(&shipped),
    };
}

/// Whether the plugin's cache in the home holds an entry for `file`,
/// of at most `cap` bytes, compiled by `engine`.
pub fn cached(engine: &Engine, file: &Path, cap: usize) -> bool {
    let Ok(bytes) = read(file, cap) else {
        return false;
    };
    let name = format!("{}.{EXTENSION}", key(engine, &bytes));
    folder_of(file).is_some_and(|folder| folder.join(name).is_file())
}

/// The bytes of `file`, refused before any is read when it holds more
/// than `cap`.
fn read(file: &Path, cap: usize) -> Result<Vec<u8>, Fault> {
    let unread = |e: std::io::Error| {
        Fault::Load(format!("{}: {e}", file.display()))
    };
    let size = std::fs::metadata(file).map_err(unread)?.len();
    if size > cap as u64 {
        return Err(Fault::Load(format!(
            "plugin.wasm has {size} bytes, the most is {cap}"
        )));
    }
    let mut bytes = Vec::new();
    // The file may have grown since its size was read.
    std::fs::File::open(file)
        .map_err(unread)?
        .take(cap as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(unread)?;
    match bytes.len() > cap {
        true => Err(Fault::Load(format!(
            "plugin.wasm has more than {cap} bytes"
        ))),
        false => Ok(bytes),
    }
}

/// The component in `file`, of at most `cap` bytes, from the plugin's
/// cache in the home when an entry for this file, engine and
/// configuration is there and can be read, and compiled otherwise. A
/// compiled one is kept, and replaces the entries of other files.
pub fn component(
    engine: &Engine,
    file: &Path,
    cap: usize,
) -> Result<(Compiled, Origin), Fault> {
    let bytes = read(file, cap)?;
    let folder = folder_of(file);
    let name = format!("{}.{EXTENSION}", key(engine, &bytes));
    if let Some(entry) = folder.as_ref().map(|f| f.join(&name))
        && entry.is_file()
    {
        // SAFETY: the folder is the host's own, in the home and
        // never in the plugin's folder, so a plugin cannot place an
        // entry in it. The entry was written by this function from a
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
    if let Some(folder) = folder
        && let Err(why) = keep(&folder, &name, &component)
    {
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
