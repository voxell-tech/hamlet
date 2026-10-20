//! The folders WASM plugins are read from: the bundle, a folder for
//! each plugin that comes with Hamlet, and the plugins folder in the
//! home, a folder for each installed plugin. Each plugin's folder
//! holds its `plugin.toml` and its `plugin.wasm`. They are read at
//! start and looked at again every `[wasm] watch` seconds, so a
//! folder put there while the engine runs is noticed, and one taken
//! away takes its plugin with it.

use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use bevy::app::{App, Startup, Update};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::message::MessageWriter;
use bevy::ecs::query::With;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Local, Query, Res};
use bevy::time::common_conditions::on_real_timer;
use hamlet_engine_api::Manifest;
use hamlet_engine_core::tuning::Tuning;
use hamlet_engine_core::{Bundle, Home, LogLine};
use hamlet_engine_grants::never_granted;
use hamlet_engine_plugins::{PluginInfo, PluginKind};

use crate::WasmCommandsExt;
use crate::add::Adding;
use crate::host::WasmTuning;

/// The plugins folder's name in the home.
pub const FOLDER: &str = "plugins";
/// A plugin's manifest, in its folder.
pub const MANIFEST: &str = "plugin.toml";
/// A plugin's component, in its folder.
pub const WASM: &str = "plugin.wasm";

pub(crate) fn plugin(app: &mut App) {
    let every = Duration::from_secs_f32(WasmTuning::designed().watch);
    app.add_systems(Startup, scan)
        .add_systems(Update, watch.run_if(on_real_timer(every)));
}

/// On a WASM plugin: its folder, in the bundle or the home.
#[derive(Component, Clone, Debug)]
pub struct Package {
    pub folder: PathBuf,
}

/// The size and the time of a file, as last looked at.
pub(crate) type Look = (u64, SystemTime);

/// A folder plugins are read from, and the kind of plugin it holds.
struct Source {
    folder: PathBuf,
    kind: PluginKind,
}

/// Adds a plugin for each folder in the bundle and the plugins
/// folder; the grants decide whether it is switched on. A folder that
/// is not a plugin, and a bundle that is not there, are told in the
/// log.
fn scan(
    home: Res<Home>,
    bundle: Option<Res<Bundle>>,
    tuning: Res<WasmTuning>,
    mut log: MessageWriter<LogLine>,
    mut commands: Commands,
) {
    if let Some(bundle) = &bundle
        && !bundle.root().is_dir()
    {
        log.write(LogLine::warn(
            "plugins",
            format!(
                "{} is not there, so no plugin that comes with \
                 Hamlet is loaded",
                bundle.root().display()
            ),
        ));
    }
    let mut bundled = Vec::new();
    for source in Source::all(&home, bundle.as_deref()) {
        for folder in source.plugin_folders() {
            let read = read_manifest(&folder, tuning.manifest_bytes)
                .and_then(|manifest| {
                    if source.kind == PluginKind::Installed
                        && bundled.contains(&manifest.id)
                    {
                        return Err(format!(
                            "{} is the id of a plugin that comes \
                             with Hamlet",
                            manifest.id
                        ));
                    }
                    Ok(manifest)
                });
            match read {
                Ok(manifest) => {
                    if source.kind == PluginKind::Bundled {
                        bundled.push(manifest.id.clone());
                    }
                    commands.spawn_plugin(
                        manifest,
                        folder,
                        source.kind,
                    );
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
}

/// Adds the plugin of each folder that appeared since the last look,
/// once its `plugin.wasm` stayed the same from one look to the next,
/// so one still being copied in is not read. Takes away each plugin
/// whose folder is gone.
fn watch(
    home: Res<Home>,
    bundle: Option<Res<Bundle>>,
    tuning: Res<WasmTuning>,
    packages: Query<(Entity, &Package, &PluginInfo)>,
    adding: Query<(), With<Adding>>,
    mut looked: Local<
        HashMap<PathBuf, ((Option<Look>, Option<Look>), bool)>,
    >,
    mut log: MessageWriter<LogLine>,
    mut commands: Commands,
) {
    // `looked` holds, for each new folder, its `plugin.wasm` and its
    // `plugin.toml` as last looked at, and whether that was told of.
    if !adding.is_empty() {
        return;
    }
    for (plugin, package, info) in &packages {
        if !package.folder.is_dir() {
            commands.entity(plugin).despawn();
            log.write(LogLine::info(
                "plugins",
                format!(
                    "{}'s folder is gone, and it with it",
                    info.name
                ),
            ));
        }
    }
    let folders = Source::all(&home, bundle.as_deref())
        .into_iter()
        .flat_map(|source| {
            source
                .plugin_folders()
                .into_iter()
                .map(move |folder| (folder, source.kind))
        })
        .collect::<Vec<_>>();
    looked.retain(|folder, _| {
        folders.iter().any(|(listed, _)| listed == folder)
    });
    for (folder, kind) in folders {
        if packages
            .iter()
            .any(|(_, package, _)| package.folder == folder)
        {
            continue;
        }
        let now =
            (look(&folder.join(WASM)), look(&folder.join(MANIFEST)));
        let before = looked.insert(folder.clone(), (now, false));
        match before {
            Some((then, told)) if then == now => {
                looked.insert(folder.clone(), (now, true));
                if told {
                    continue;
                }
            }
            _ => continue,
        }
        let read = match now.0 {
            Some(_) => read_manifest(&folder, tuning.manifest_bytes),
            None => Err(format!(
                "it has no {WASM}, so it is not a built plugin"
            )),
        }
        .and_then(|manifest| {
            let bundled = packages.iter().any(|(_, _, info)| {
                info.kind == PluginKind::Bundled
                    && info.id == manifest.id
            });
            if kind == PluginKind::Installed && bundled {
                return Err(format!(
                    "{} is the id of a plugin that comes with Hamlet",
                    manifest.id
                ));
            }
            Ok(manifest)
        });
        match read {
            Ok(manifest) => {
                log.write(LogLine::info(
                    "plugins",
                    format!(
                        "{} is in {}",
                        manifest.name,
                        folder.display()
                    ),
                ));
                // One that comes with Hamlet takes the place of one
                // installed of its id.
                for (plugin, _, info) in &packages {
                    if kind == PluginKind::Bundled
                        && info.kind == PluginKind::Installed
                        && info.id == manifest.id
                    {
                        commands.entity(plugin).despawn();
                    }
                }
                commands.spawn_plugin(manifest, folder, kind);
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

impl Source {
    /// The bundle, when there is one, then the plugins folder in
    /// `home`.
    fn all(home: &Home, bundle: Option<&Bundle>) -> Vec<Self> {
        let bundled = bundle.map(|bundle| Self {
            folder: bundle.root().to_path_buf(),
            kind: PluginKind::Bundled,
        });
        let installed = Self {
            folder: home.path(FOLDER),
            kind: PluginKind::Installed,
        };
        bundled.into_iter().chain([installed]).collect()
    }

    /// The folders here a plugin may be in, in order.
    fn plugin_folders(&self) -> Vec<PathBuf> {
        let Ok(entries) = fs::read_dir(&self.folder) else {
            return Vec::new();
        };
        let mut folders = entries
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| is_plugin_folder(path))
            .collect::<Vec<_>>();
        folders.sort();
        folders
    }
}

/// The size and time of the file at `path`, if there is one.
pub(crate) fn look(path: &Path) -> Option<Look> {
    let meta = fs::symlink_metadata(path).ok()?;
    Some((meta.len(), meta.modified().ok()?))
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
    let table = toml::from_str::<toml::Table>(&text)
        .map_err(|e| format!("{MANIFEST}: {}", e.message()))?;
    let asked = table
        .get("asks")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|ask| ask.get("capability")?.as_str());
    for name in asked {
        if let Some(power) = never_granted(name) {
            return Err(format!(
                "{MANIFEST} asks for {name}, and {power} is never \
                 granted"
            ));
        }
    }
    let manifest = table
        .try_into::<Manifest>()
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
    let (file, name) = open_capped(path, most)?;
    let mut bytes = Vec::new();
    // No further than the cap, in case it grew since.
    file.take(most + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("{name} cannot be read: {e}"))?;
    if bytes.len() as u64 > most {
        return Err(format!("{name} has more than {most} bytes"));
    }
    Ok(bytes)
}

/// Copies the plain file `from` to `to`, refused as [`read_capped`]
/// refuses it.
pub(crate) fn copy_capped(
    from: &Path,
    to: &Path,
    most: u64,
) -> Result<(), String> {
    let (file, name) = open_capped(from, most)?;
    let unwritten =
        |e: std::io::Error| format!("{name} cannot be copied: {e}");
    let mut out = fs::File::create(to).map_err(unwritten)?;
    let copied = std::io::copy(&mut file.take(most + 1), &mut out)
        .map_err(unwritten)?;
    if copied > most {
        return Err(format!("{name} has more than {most} bytes"));
    }
    out.sync_all().map_err(unwritten)
}

/// The plain file `path` opened, and its name, refused before any of
/// it is read when it is a link, not a plain file, or longer than
/// `most`.
pub(crate) fn open_capped(
    path: &Path,
    most: u64,
) -> Result<(fs::File, String), String> {
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
    Ok((file, name))
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
