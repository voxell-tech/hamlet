//! Removing a native plugin: its entity and its files.

use bevy::prelude::*;
use hamlet_base::HomeDir;
use hamlet_sdk::capability::{Removed, Store};
use hamlet_sdk::command::Notice;
use hamlet_sdk::plugin::{FullAccess, PluginInfo};

use crate::guard::Settling;
use crate::load::Library;
use crate::{cache, marker};

/// Despawns the plugin the chief removed, which takes what it added
/// to the window, and deletes its folder, or only its manifest and
/// library when the chief keeps its data. Its own systems keep
/// running, and its library stays loaded, until Hamlet starts again.
pub fn removed(
    removed: On<Removed>,
    plugins: Query<(Entity, &PluginInfo, &Library), With<FullAccess>>,
    home: Res<HomeDir>,
    mut settling: ResMut<Settling>,
    store: Option<ResMut<Store>>,
    mut commands: Commands,
) {
    let found = plugins
        .iter()
        .find(|(_, info, _)| info.id == removed.plugin);
    let Some((plugin, info, library)) = found else {
        return;
    };
    // The folder is the plugin's own, named for its id.
    if library.folder.file_name().and_then(|n| n.to_str())
        != Some(info.id.as_str())
    {
        warn!(
            "native plugin {}: not removed, its folder is not its \
             own",
            info.id
        );
        return;
    }
    // Clearing a link would delete the files it points at.
    let linked = std::fs::symlink_metadata(&library.folder)
        .is_ok_and(|meta| meta.file_type().is_symlink());
    if linked {
        let why = format!(
            "{} was not removed: its folder is a link. Remove it by \
             hand.",
            info.name
        );
        warn!("native plugin {}: {why}", info.id);
        commands.trigger(Notice(why));
        return;
    }
    let cleared = if removed.keep_data {
        [library.file.clone(), library.folder.join("manifest.toml")]
            .iter()
            .try_for_each(std::fs::remove_file)
    } else {
        std::fs::remove_dir_all(&library.folder)
    };
    if let Err(why) = cleared {
        warn!(
            "native plugin {}: not all of it was removed: {why}",
            info.id
        );
    }
    cache::clear(&home.0.root, &info.id);
    commands.entity(plugin).despawn();
    if let Some(mut store) = store {
        store.remove(&info.id);
    }
    marker::settled(&home.0.root, &info.id);
    settling.forget(&info.id);
    info!(
        "native plugin {}: removed; its systems run until Hamlet \
         starts again",
        info.id
    );
}
