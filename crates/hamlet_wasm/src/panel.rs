//! The panels of an installed plugin, as entities of the dock.

use bevy::prelude::*;
use hamlet_sdk::plugin::Entry;

use crate::host::Declared;
use crate::view;

/// The id of a plugin's panel in the dock. Prefixed with the plugin's
/// id, so that no plugin can take the id of another panel.
pub fn dock_id(plugin: &str, panel: &str) -> String {
    format!("{plugin}/{panel}")
}

/// The id a plugin knows its panel by, from the dock's id.
pub fn local_id<'a>(
    plugin: &str,
    dock_id: &'a str,
) -> Option<&'a str> {
    dock_id.strip_prefix(plugin)?.strip_prefix('/')
}

/// Spawns the panels `declared` as contributions of `plugin`, and
/// returns their ids in the dock.
pub fn spawn(
    plugin: Entity,
    id: &str,
    declared: &[Declared],
    commands: &mut Commands,
) -> Vec<String> {
    let mut entry = Entry::default();
    let mut ids = Vec::new();
    for panel in declared {
        let dock = dock_id(id, &panel.id);
        entry.panel_by_id(
            &dock,
            &panel.title,
            panel.icon,
            view::panel,
        );
        ids.push(dock);
    }
    entry.spawn(plugin, commands);
    ids
}
