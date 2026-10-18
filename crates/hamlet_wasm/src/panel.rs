//! The panels of an installed plugin, as entities of the dock.

use bevy::prelude::*;
use hamlet_sdk::dock::Panel;
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

/// Makes the panels of `plugin` the ones `declared`, given the `held`
/// ones it has: a panel it still declares stays where it is in the
/// dock, one it no longer declares goes and a new one is added.
/// Returns the ids of all of them in the dock.
pub fn replace(
    plugin: Entity,
    id: &str,
    held: &[(Entity, &Panel)],
    declared: &[Declared],
    commands: &mut Commands,
) -> Vec<String> {
    let wanted = declared
        .iter()
        .map(|panel| dock_id(id, &panel.id))
        .collect::<Vec<_>>();
    for (entity, panel) in held {
        if !wanted.contains(&panel.id) {
            commands.entity(*entity).despawn();
        }
    }
    let fresh = declared
        .iter()
        .filter(|panel| {
            let dock = dock_id(id, &panel.id);
            !held.iter().any(|(_, held)| held.id == dock)
        })
        .cloned()
        .collect::<Vec<_>>();
    spawn(plugin, id, &fresh, commands);
    wanted
}
