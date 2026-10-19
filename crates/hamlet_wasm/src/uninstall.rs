//! Removing an installed plugin: its entity and its files, and its
//! data if the chief said so.

use std::path::Path;

use bevy::prelude::*;
use hamlet_sdk::capability::{Removed, Store};
use hamlet_sdk::plugin::PluginInfo;

use crate::db::DB;
use crate::files::FILES;
use crate::reload::Watch;
use crate::storage::DATA;

/// Despawns the plugin the chief removed, deletes its files, and the
/// folders of its data and files unless the chief kept it, and drops
/// its record of choices.
pub fn removed(
    removed: On<Removed>,
    plugins: Query<(Entity, &PluginInfo, &Watch)>,
    store: Option<ResMut<Store>>,
    mut commands: Commands,
) {
    let found = plugins.iter().find(|(_, info, _)| {
        info.id == removed.plugin && !info.builtin
    });
    let Some((plugin, info, watch)) = found else {
        return;
    };
    // The folder is the plugin's own, named for its id.
    if watch.folder.file_name().and_then(|n| n.to_str())
        != Some(info.id.as_str())
    {
        warn!(
            "plugin {}: not removed, its folder is not its own",
            info.id
        );
        return;
    }
    if let Err(why) = clear(&watch.folder, removed.keep_data) {
        warn!("plugin {}: not all of it was removed: {why}", info.id);
    }
    crate::cache::drop_cache(&watch.folder);
    commands.entity(plugin).despawn();
    if let Some(mut store) = store {
        store.remove(&info.id);
    }
    let kept = if removed.keep_data {
        ", its data kept"
    } else {
        ""
    };
    info!("plugin {}: removed{kept}", info.id);
}

/// Deletes the folder, or all that is in it but its data and files
/// when `keep_data`.
fn clear(folder: &Path, keep_data: bool) -> std::io::Result<()> {
    if !keep_data {
        return std::fs::remove_dir_all(folder);
    }
    for entry in std::fs::read_dir(folder)? {
        let path = entry?.path();
        if path.file_name().is_some_and(|name| {
            name == DATA || name == FILES || name == DB
        }) {
            continue;
        }
        match path.is_dir() {
            true => std::fs::remove_dir_all(&path)?,
            false => std::fs::remove_file(&path)?,
        }
    }
    Ok(())
}
