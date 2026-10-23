//! Plugins' components kept compiled in the engine's home, so a
//! plugin starts without compiling until its `plugin.wasm` or the
//! engine changes.

use std::fs;
use std::path::{Path, PathBuf};

use hamlet_engine_core::{Home, write_whole};
use hamlet_engine_plugins::PluginInfo;
use sha2::{Digest, Sha256};
use wasmtime::Engine;
use wasmtime::component::Component;

/// The folder of the home holding a folder of compiled components for
/// each plugin, by its id.
const COMPILED: &str = "compiled";

/// The WASM host's methods on [`Home`].
pub(crate) trait WasmHomeExt {
    /// The folder the component of `info` is kept compiled in.
    fn kept_compiled(&self, info: &PluginInfo) -> PathBuf;
}

impl WasmHomeExt for Home {
    fn kept_compiled(&self, info: &PluginInfo) -> PathBuf {
        self.path(COMPILED).join(info.id.as_str())
    }
}

/// The component of `bytes`: the one kept in `folder`, else compiled
/// and kept there in place of any other.
pub(crate) fn component(
    engine: &Engine,
    bytes: &[u8],
    folder: &Path,
) -> Result<Component, String> {
    let name = format!("{:x}.cwasm", Sha256::digest(bytes));
    let kept = folder.join(&name);
    if kept.is_file() {
        // SAFETY: the file is named by the hash of the bytes it was
        // compiled from, and only this function writes the folder;
        // wasmtime refuses one of another version or configuration,
        // which is then compiled again.
        if let Ok(component) =
            unsafe { Component::deserialize_file(engine, &kept) }
        {
            return Ok(component);
        }
    }
    let component = Component::new(engine, bytes)
        .map_err(|e| format!("{e:#}"))?;
    // One that cannot be kept is compiled again next time.
    if let Ok(serialized) = component.serialize()
        && write_whole(&kept, serialized).is_ok()
    {
        forget_others(folder, &name);
    }
    Ok(component)
}

/// Takes away every file in `folder` but `name`.
fn forget_others(folder: &Path, name: &str) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_name() != name {
            let _ = fs::remove_file(entry.path());
        }
    }
}
