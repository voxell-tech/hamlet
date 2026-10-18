//! Switching a plugin on and off.

use bevy::ecs::entity::Entity;
use bevy::ecs::event::Event;
use bevy::ecs::observer::On;
use bevy::ecs::system::{Commands, Query};

use super::{Contributions, EntryPoint, PluginInfo, Status};

/// Enables the plugin of this id.
#[derive(Event)]
pub struct Enable(pub String);

/// Disables the plugin of this id: what it added is despawned.
#[derive(Event)]
pub struct Disable(pub String);

pub(super) fn enable(
    enable: On<Enable>,
    plugins: Query<(Entity, &PluginInfo, &Status, &EntryPoint)>,
    mut commands: Commands,
) {
    for (plugin, info, status, point) in &plugins {
        if info.id == enable.0 && *status == Status::Disabled {
            commands.entity(plugin).insert(Status::Starting);
            point.run(plugin, &mut commands);
        }
    }
}

pub(super) fn disable(
    disable: On<Disable>,
    plugins: Query<(Entity, &PluginInfo, &Status)>,
    mut commands: Commands,
) {
    for (plugin, info, status) in &plugins {
        if info.id == disable.0 && *status != Status::Disabled {
            commands
                .entity(plugin)
                .despawn_related::<Contributions>()
                .insert(Status::Disabled);
        }
    }
}
