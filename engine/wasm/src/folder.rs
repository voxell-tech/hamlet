//! The plugins folder in the home: a folder for each installed
//! plugin, with its `plugin.toml` and its `plugin.wasm`.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use bevy::app::{App, Startup};
use bevy::ecs::component::Component;
use bevy::ecs::message::MessageWriter;
use bevy::ecs::system::{Commands, Res};
use hamlet_engine_api::Manifest;
use hamlet_engine_core::{Home, LogLine};
use hamlet_engine_plugins::{Enable, PluginInfo};

use crate::host::WasmTuning;

/// The plugins folder's name in the home.
pub const FOLDER: &str = "plugins";
/// A plugin's manifest, in its folder.
pub const MANIFEST: &str = "plugin.toml";
/// A plugin's component, in its folder.
pub const WASM: &str = "plugin.wasm";

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Startup, scan);
}

/// On an installed plugin: its folder.
#[derive(Component, Clone, Debug)]
pub struct Package {
    pub folder: PathBuf,
}

/// Adds a plugin for each folder in the plugins folder and switches
/// it on. A folder that is not a plugin is told in the log.
fn scan(
    home: Res<Home>,
    tuning: Res<WasmTuning>,
    mut log: MessageWriter<LogLine>,
    mut commands: Commands,
) {
    let Ok(entries) = fs::read_dir(home.path(FOLDER)) else {
        return;
    };
    let mut folders = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| is_plugin_folder(path))
        .collect::<Vec<_>>();
    folders.sort();
    for folder in folders {
        match read_manifest(&folder, tuning.manifest_bytes) {
            Ok(manifest) => {
                let plugin = commands
                    .spawn((
                        PluginInfo {
                            id: manifest.id,
                            name: manifest.name,
                            version: manifest.version,
                            builtin: false,
                        },
                        Package { folder },
                    ))
                    .id();
                commands.trigger(Enable { entity: plugin });
            }
            Err(why) => {
                log.write(LogLine::error(
                    "plugins",
                    format!("{}: {why}", folder.display()),
                ));
            }
        }
    }
}

/// Whether `path` is a folder a plugin may be in: a folder, not a
/// link, whose name does not start with a dot.
pub fn is_plugin_folder(path: &Path) -> bool {
    let named = path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| !name.starts_with('.'));
    named
        && fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir())
}

/// The manifest in `folder`, which the engine can take.
pub fn read_manifest(
    folder: &Path,
    most: u64,
) -> Result<Manifest, String> {
    let bytes = read_capped(&folder.join(MANIFEST), most)?;
    let text = String::from_utf8(bytes)
        .map_err(|_| format!("{MANIFEST} is not text"))?;
    let manifest = toml::from_str::<Manifest>(&text)
        .map_err(|e| format!("{MANIFEST}: {}", e.message()))?;
    manifest
        .check()
        .map_err(|why| format!("{MANIFEST}: {why}"))?;
    Ok(manifest)
}

/// The bytes of the plain file `path`, refused before any is read
/// when it is a link, not a plain file, or longer than `most`.
pub fn read_capped(
    path: &Path,
    most: u64,
) -> Result<Vec<u8>, String> {
    let name = path.file_name().map_or_else(String::new, |name| {
        name.to_string_lossy().into()
    });
    let unread =
        |e: std::io::Error| format!("{name} cannot be read: {e}");
    let meta = fs::symlink_metadata(path).map_err(unread)?;
    if meta.is_symlink() {
        return Err(format!("{name} is a link"));
    }
    if !meta.is_file() {
        return Err(format!("{name} is not a plain file"));
    }
    if meta.len() > most {
        return Err(format!(
            "{name} has {} bytes, and the most is {most}",
            meta.len()
        ));
    }
    let file = fs::File::open(path).map_err(unread)?;
    if !same(&meta, &file.metadata().map_err(unread)?) {
        return Err(format!("{name} changed while it was read"));
    }
    let mut bytes = Vec::new();
    // No further than the cap, in case it grew since.
    file.take(most + 1)
        .read_to_end(&mut bytes)
        .map_err(unread)?;
    if bytes.len() as u64 > most {
        return Err(format!("{name} has more than {most} bytes"));
    }
    Ok(bytes)
}

/// Whether a file looked at by its path and the file opened are one,
/// so a link put in between is not followed.
fn same(looked: &fs::Metadata, opened: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        opened.is_file()
            && looked.dev() == opened.dev()
            && looked.ino() == opened.ino()
    }
    #[cfg(not(unix))]
    {
        opened.is_file() && looked.len() == opened.len()
    }
}
