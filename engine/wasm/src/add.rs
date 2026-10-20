//! ADD PLUGIN... and uninstalling. The person chooses the folder of
//! a built plugin in the system's dialog; it is checked, copied into
//! the plugins folder, and the person is asked about it at once.
//! Uninstalling a plugin takes its folder away.

use std::fs;
use std::path::{Path, PathBuf};

use bevy::app::{App, Update};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::event::Event;
use bevy::ecs::message::MessageWriter;
use bevy::ecs::observer::On;
use bevy::ecs::system::{Commands, Query, Res};
use bevy::tasks::futures::check_ready;
use bevy::tasks::{IoTaskPool, Task};
use hamlet_engine_api::Manifest;
use hamlet_engine_core::{Home, LogLine};
use hamlet_engine_plugins::{
    AddPlugin, PluginInfo, PluginKind, Uninstall,
};

use crate::WasmCommandsExt;
use crate::dialog::{Asker, ChooseFolder};
use crate::folder::{
    FOLDER, MANIFEST, Package, WASM, copy_capped, open_capped,
    read_manifest,
};
use crate::host::WasmTuning;

pub(crate) fn plugin(app: &mut App) {
    app.add_observer(ask)
        .add_observer(add_from)
        .add_observer(uninstall)
        .add_systems(Update, added);
}

/// Adds the built plugin in `folder`, as the folder the person chose
/// in the dialog is.
#[derive(Event, Clone, Debug)]
pub struct AddFrom {
    pub folder: PathBuf,
}

/// On an entity of its own: a plugin's files being copied in, off the
/// frame, into the folder it is to be in.
#[derive(Component)]
pub(crate) struct Adding(Task<Result<(Manifest, PathBuf), String>>);

/// A built plugin's folder that may be added.
#[derive(Debug)]
pub(crate) struct Checked {
    pub(crate) manifest: Manifest,
    /// The folder, made canonical.
    pub(crate) folder: PathBuf,
}

fn ask(_: On<AddPlugin>, mut commands: Commands) {
    commands.trigger(ChooseFolder {
        title: "Choose the folder of a built plugin".into(),
        asker: Asker::Adding,
    });
}

/// Checks the folder to add from, and copies it in off the frame.
fn add_from(
    event: On<AddFrom>,
    home: Res<Home>,
    tuning: Res<WasmTuning>,
    plugins: Query<(&PluginInfo, Option<&Package>)>,
    mut log: MessageWriter<LogLine>,
    mut commands: Commands,
) {
    let checked = check(&event.folder, home.root(), &tuning)
        .and_then(|checked| {
            let id = &checked.manifest.id;
            match plugins.iter().find(|(info, _)| info.id == *id) {
                // An update goes where the plugin is.
                Some((info, Some(package)))
                    if info.kind == PluginKind::Installed =>
                {
                    Ok((checked, package.folder.clone()))
                }
                Some((info, _))
                    if info.kind == PluginKind::Bundled =>
                {
                    Err(format!(
                        "{id} is the id of a plugin that comes with \
                         Hamlet"
                    ))
                }
                Some(_) => Err(format!(
                    "{id} is the id of a plugin built into the \
                     engine"
                )),
                None => {
                    let into = home.path(FOLDER).join(id.as_str());
                    Ok((checked, into))
                }
            }
        });
    let (checked, into) = match checked {
        Ok(checked) => checked,
        Err(why) => {
            log.write(LogLine::error(
                "plugins",
                format!(
                    "{} cannot be added: {why}",
                    event.folder.display()
                ),
            ));
            return;
        }
    };
    let plugins = home.path(FOLDER);
    let tuning = tuning.clone();
    commands.spawn(Adding(IoTaskPool::get().spawn(async move {
        checked
            .place(&plugins, &into, &tuning)
            .map(|manifest| (manifest, into))
    })));
}

/// Adds each plugin whose files took their place, in place of one
/// that was there, and asks the person about it.
fn added(
    mut adding: Query<(Entity, &mut Adding)>,
    plugins: Query<(Entity, &PluginInfo)>,
    mut log: MessageWriter<LogLine>,
    mut commands: Commands,
) {
    for (task, mut placing) in &mut adding {
        let Some(placed) = check_ready(&mut placing.0) else {
            continue;
        };
        commands.entity(task).despawn();
        match placed {
            Ok((manifest, folder)) => {
                for (plugin, info) in &plugins {
                    if info.id == manifest.id
                        && info.kind == PluginKind::Installed
                    {
                        commands.entity(plugin).despawn();
                    }
                }
                log.write(LogLine::info(
                    "plugins",
                    format!(
                        "{} {} is added",
                        manifest.name, manifest.version
                    ),
                ));
                commands.spawn_plugin(
                    manifest,
                    folder,
                    PluginKind::Installed,
                );
            }
            Err(why) => {
                log.write(LogLine::error(
                    "plugins",
                    format!("The plugin cannot be added: {why}"),
                ));
            }
        }
    }
}

/// Takes an installed plugin away, and its folder. One that comes
/// with Hamlet stays.
fn uninstall(
    event: On<Uninstall>,
    plugins: Query<(&PluginInfo, &Package)>,
    mut log: MessageWriter<LogLine>,
    mut commands: Commands,
) {
    let Ok((info, package)) = plugins.get(event.entity) else {
        return;
    };
    if info.kind != PluginKind::Installed {
        log.write(LogLine::warn(
            "plugins",
            format!(
                "{} comes with Hamlet and is not uninstalled; it \
                 can be switched off",
                info.name
            ),
        ));
        return;
    }
    commands.entity(event.entity).despawn();
    // A link is taken away itself, not what it points at.
    let line = match fs::remove_dir_all(&package.folder) {
        Ok(()) => LogLine::info(
            "plugins",
            format!("{} is uninstalled", info.name),
        ),
        Err(why) => LogLine::error(
            "plugins",
            format!(
                "{} is uninstalled, and {} cannot be taken away: \
                 {why}",
                info.name,
                package.folder.display()
            ),
        ),
    };
    log.write(line);
}

/// The folder `chosen`, when it holds a built plugin that may be
/// added to the home `home`: a `plugin.toml` the engine can take and
/// a `plugin.wasm`, each a plain file within its size, and the folder
/// not in the home nor holding it.
pub(crate) fn check(
    chosen: &Path,
    home: &Path,
    tuning: &WasmTuning,
) -> Result<Checked, String> {
    let folder = chosen
        .canonicalize()
        .map_err(|_| "the folder cannot be read".to_string())?;
    if !folder.is_dir() {
        return Err("what was chosen is not a folder".into());
    }
    let home =
        home.canonicalize().unwrap_or_else(|_| home.to_path_buf());
    if folder.starts_with(&home) || home.starts_with(&folder) {
        return Err("the folder is in the engine's home, or holds \
                    it"
        .into());
    }
    let manifest = read_manifest(&folder, tuning.manifest_bytes)?;
    if !folder.join(WASM).exists() {
        return Err(format!(
            "it has no {WASM}, so it is not a built plugin"
        ));
    }
    open_capped(&folder.join(WASM), tuning.wasm_bytes)?;
    Ok(Checked { manifest, folder })
}

impl Checked {
    /// Copies the plugin into the folder `into` of the plugins folder
    /// `plugins`: into a folder of its own there first, read again as
    /// the engine reads a plugin, then moved into place. What it
    /// holds is the plugin the person chose, unless that changed
    /// meanwhile.
    pub(crate) fn place(
        &self,
        plugins: &Path,
        into: &Path,
        tuning: &WasmTuning,
    ) -> Result<Manifest, String> {
        let id = &self.manifest.id;
        let stage = plugins.join(format!(".adding_{id}"));
        let _ = fs::remove_dir_all(&stage);
        fs::create_dir_all(&stage).map_err(|e| {
            format!("the plugins folder cannot be written: {e}")
        })?;
        let placed = (|| {
            copy_capped(
                &self.folder.join(MANIFEST),
                &stage.join(MANIFEST),
                tuning.manifest_bytes,
            )?;
            copy_capped(
                &self.folder.join(WASM),
                &stage.join(WASM),
                tuning.wasm_bytes,
            )?;
            let copied =
                read_manifest(&stage, tuning.manifest_bytes)?;
            if copied.id != *id
                || copied.version != self.manifest.version
            {
                return Err("the folder changed while it was added"
                    .to_string());
            }
            put(&stage, into)?;
            Ok(copied)
        })();
        let _ = fs::remove_dir_all(&stage);
        placed
    }
}

/// Moves the folder `stage` to `into`, over the folder there.
fn put(stage: &Path, into: &Path) -> Result<(), String> {
    let moved = |e: std::io::Error| {
        format!("the plugin cannot take its place: {e}")
    };
    match fs::symlink_metadata(into) {
        Err(_) => fs::rename(stage, into).map_err(moved),
        Ok(meta) if meta.is_dir() => {
            let name =
                into.file_name().map_or_else(String::new, |name| {
                    name.to_string_lossy().into()
                });
            let old =
                into.with_file_name(format!(".removing_{name}"));
            let _ = fs::remove_dir_all(&old);
            fs::rename(into, &old).map_err(moved)?;
            if let Err(e) = fs::rename(stage, into) {
                let _ = fs::rename(&old, into);
                return Err(moved(e));
            }
            let _ = fs::remove_dir_all(&old);
            Ok(())
        }
        Ok(_) => Err(format!(
            "{} is not a folder; take it away by hand",
            into.display()
        )),
    }
}
