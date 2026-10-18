//! The panels of an installed plugin, as entities of the dock.

use bevy::prelude::*;
use hamlet_sdk::plugin::Entry;
use hamlet_sdk::tree::id::dock_id;
use hamlet_sdk::tree::view;

use crate::host::Declared;

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
