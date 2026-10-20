//! Switching a plugin on and off, and what links what it adds to it.

use bevy::app::App;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::event::EntityEvent;
use bevy::ecs::lifecycle::Add;
use bevy::ecs::message::MessageWriter;
use bevy::ecs::observer::On;
use bevy::ecs::query::{Has, With};
use bevy::ecs::system::{Commands, Query, Res};
use hamlet_engine_core::{Budgets, LogLine};

use crate::PluginInfo;
use crate::entry::{Entry, EntryPoint};

pub(crate) fn plugin(app: &mut App) {
    app.add_message::<LogLine>()
        .add_observer(one_of_each)
        .add_observer(enable)
        .add_observer(disable);
}

/// On what a plugin added: the plugin.
#[derive(Component, Clone, Copy, Debug)]
#[relationship(relationship_target = Contributions)]
pub struct ContributedBy(pub Entity);

/// On a plugin: everything it added, which goes when it does.
#[derive(Component, Debug)]
#[relationship_target(relationship = ContributedBy, linked_spawn)]
pub struct Contributions(Vec<Entity>);

/// On a plugin while it is switched on.
#[derive(Component, Clone, Copy, Debug)]
pub struct Enabled;

/// Switches the plugin on: its entry point runs, and what it declares
/// is added.
#[derive(EntityEvent, Clone, Copy, Debug)]
pub struct Enable {
    pub entity: Entity,
}

/// Switches the plugin off: what it added is despawned.
#[derive(EntityEvent, Clone, Copy, Debug)]
pub struct Disable {
    pub entity: Entity,
}

/// Turns away a plugin whose id another has already.
fn one_of_each(
    added: On<Add, PluginInfo>,
    plugins: Query<(Entity, &PluginInfo)>,
    mut log: MessageWriter<LogLine>,
    mut commands: Commands,
) {
    let Ok((_, new)) = plugins.get(added.entity) else {
        return;
    };
    let taken = plugins.iter().any(|(other, info)| {
        other != added.entity && info.id == new.id
    });
    if taken {
        log.write(LogLine::error(
            "plugins",
            format!(
                "{} is added already; the second is turned away",
                new.id
            ),
        ));
        commands.entity(added.entity).despawn();
    }
}

fn enable(
    event: On<Enable>,
    plugins: Query<(&PluginInfo, &EntryPoint, Has<Enabled>)>,
    budgets: Res<Budgets>,
    mut log: MessageWriter<LogLine>,
    mut commands: Commands,
) {
    let plugin = event.entity;
    let Ok((info, point, on)) = plugins.get(plugin) else {
        return;
    };
    if on {
        return;
    }
    let mut entry = Entry::default();
    (point.0)(&mut entry);
    for why in entry.spawn(plugin, &info.id, &budgets, &mut commands)
    {
        log.write(LogLine::warn(info.id.as_str(), why));
    }
    commands.entity(plugin).insert(Enabled);
    log.write(LogLine::info(
        "plugins",
        format!("{} is on", info.name),
    ));
}

fn disable(
    event: On<Disable>,
    plugins: Query<&PluginInfo, With<Enabled>>,
    mut log: MessageWriter<LogLine>,
    mut commands: Commands,
) {
    let plugin = event.entity;
    let Ok(info) = plugins.get(plugin) else {
        return;
    };
    commands
        .entity(plugin)
        .despawn_related::<Contributions>()
        .remove::<Enabled>();
    log.write(LogLine::info(
        "plugins",
        format!("{} is off", info.name),
    ));
}
