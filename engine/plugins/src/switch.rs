//! Adding a plugin built in, switching a plugin on and off, and what
//! links what it adds to it.

use bevy::app::App;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::event::EntityEvent;
use bevy::ecs::lifecycle::Add;
use bevy::ecs::message::MessageWriter;
use bevy::ecs::observer::On;
use bevy::ecs::query::{Has, With};
use bevy::ecs::system::{Commands, Query};
use bevy::ecs::world::World;
use hamlet_engine_api::Manifest;
use hamlet_engine_core::{Budgets, LogLine};
use hamlet_engine_shell::StartRank;

use crate::entry::{Entry, EntryPoint};
use crate::needs::Starting;
use crate::switched_off::SwitchedOff;
use crate::{PluginInfo, PluginKind};

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

/// On a plugin the engine stopped while it was on: why. What it added
/// stays until it is switched off.
#[derive(Component, Clone, PartialEq, Debug)]
pub struct Stopped(pub String);

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

/// Has a plugin built in start at its turn.
fn enable(
    event: On<Enable>,
    plugins: Query<Has<Enabled>, With<EntryPoint>>,
    mut commands: Commands,
) {
    if plugins.get(event.entity) == Ok(false) {
        commands.entity(event.entity).insert(Starting);
    }
}

/// The plugins' methods on [`Commands`].
pub trait PluginCommandsExt {
    /// Adds the plugin `manifest` describes, built into the engine
    /// with its `entry`, and switches it on unless the person
    /// switched it off. A manifest the engine cannot take is told
    /// in the log, and nothing is added.
    fn add_builtin(
        &mut self,
        manifest: Manifest,
        entry: fn(&mut Entry),
    ) -> Option<Entity>;

    /// Adds what `entry` declares, linked to `plugin` at its `rank`,
    /// within `budgets`, and marks the plugin [`Enabled`]. What is
    /// left out is told in the log.
    fn switch_on(
        &mut self,
        plugin: Entity,
        info: &PluginInfo,
        rank: &StartRank,
        entry: Entry,
        budgets: &Budgets,
    );
}

impl PluginCommandsExt for Commands<'_, '_> {
    fn add_builtin(
        &mut self,
        manifest: Manifest,
        entry: fn(&mut Entry),
    ) -> Option<Entity> {
        if let Err(why) = manifest.check() {
            let line = LogLine::error(
                "plugins",
                format!("{} is not added: {why}", manifest.id),
            );
            self.queue(move |world: &mut World| {
                world.write_message(line);
            });
            return None;
        }
        let id = manifest.id.clone();
        let plugin = self
            .spawn((
                PluginInfo::of(manifest, PluginKind::BuiltIn),
                EntryPoint(entry),
            ))
            .id();
        self.queue(move |world: &mut World| {
            let off = world
                .get_resource::<SwitchedOff>()
                .is_some_and(|off| off.contains(&id));
            if !off {
                world.trigger(Enable { entity: plugin });
            }
        });
        Some(plugin)
    }

    fn switch_on(
        &mut self,
        plugin: Entity,
        info: &PluginInfo,
        rank: &StartRank,
        entry: Entry,
        budgets: &Budgets,
    ) {
        for why in entry.spawn(plugin, &info.id, rank, budgets, self)
        {
            self.write_message(LogLine::warn(info.id.as_str(), why));
        }
        self.entity(plugin).insert(Enabled);
        self.write_message(LogLine::info(
            "plugins",
            format!("{} is on", info.name),
        ));
    }
}

fn disable(
    event: On<Disable>,
    plugins: Query<&PluginInfo, With<Enabled>>,
    mut log: MessageWriter<LogLine>,
    mut commands: Commands,
) {
    let plugin = event.entity;
    commands.entity(plugin).try_remove::<Starting>();
    let Ok(info) = plugins.get(plugin) else {
        return;
    };
    commands
        .entity(plugin)
        .despawn_related::<Contributions>()
        .remove::<(Enabled, Stopped)>();
    log.write(LogLine::info(
        "plugins",
        format!("{} is off", info.name),
    ));
}
