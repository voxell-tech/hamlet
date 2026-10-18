//! A plugin that the chief has not let in yet, and letting it in.

use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::event::Event;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::system::{Commands, Query};
use bevy::ecs::world::World;

use super::ask::Asks;
use super::granted::Choices;
use super::petition;
use super::record::Record;
use super::store::Store;
use crate::plugin::PluginInfo;

/// On the entity of a plugin that is in the install folder and not
/// installed: nothing of it is compiled or run.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Uninstalled;

/// The chief let the plugin of this id in. It has its choices and is
/// no longer [`Uninstalled`]; whoever holds its files brings it up.
#[derive(Event, Clone, PartialEq, Eq, Debug)]
pub struct Installed {
    pub plugin: String,
}

/// The chief wants to be asked about installing the plugin of this
/// id.
#[derive(Event, Clone, PartialEq, Eq, Debug)]
pub struct AskToInstall {
    pub plugin: String,
}

/// The chief wants to remove the plugin of this id, and is to be
/// asked what becomes of its data.
#[derive(Event, Clone, PartialEq, Eq, Debug)]
pub struct AskToRemove {
    pub plugin: String,
}

/// The chief removed the plugin of this id. Whoever holds its files
/// takes the plugin away, and its data too unless `keep_data`.
#[derive(Event, Clone, PartialEq, Eq, Debug)]
pub struct Removed {
    pub plugin: String,
    pub keep_data: bool,
}

pub(super) fn ask_to_remove(
    ask: On<AskToRemove>,
    plugins: Query<(Entity, &PluginInfo)>,
    mut commands: Commands,
) {
    let found = plugins
        .iter()
        .find(|(_, info)| info.id == ask.plugin && !info.builtin)
        .map(|(entity, _)| entity);
    if let Some(plugin) = found {
        commands.queue(move |world: &mut World| {
            petition::remove(world, plugin)
        });
    }
}

pub(super) fn ask(
    ask: On<AskToInstall>,
    plugins: Query<(Entity, &PluginInfo), With<Uninstalled>>,
    mut commands: Commands,
) {
    let found = plugins
        .iter()
        .find(|(_, info)| info.id == ask.plugin)
        .map(|(entity, _)| entity);
    if let Some(plugin) = found {
        commands.queue(move |world: &mut World| {
            petition::install(world, plugin)
        });
    }
}

/// Lets the plugin `plugin` in, allowing everything its manifest
/// asks.
pub(super) fn admit(world: &mut World, plugin: Entity) {
    let (Some(info), Some(asks)) = (
        world.get::<PluginInfo>(plugin).cloned(),
        world.get::<Asks>(plugin).cloned(),
    ) else {
        return;
    };
    let choices = Choices::allowing(&asks.capabilities());
    if let Some(mut store) = world.get_resource_mut::<Store>() {
        store.set(
            &info.id,
            Record::installed(&info.version, &choices).scoped(&asks),
        );
    }
    world
        .entity_mut(plugin)
        .insert(choices)
        .remove::<Uninstalled>();
    world.trigger(Installed { plugin: info.id });
}

/// Records that the chief said no to the plugin `plugin`.
pub(super) fn decline(world: &mut World, plugin: Entity) {
    let (Some(info), Some(asks)) =
        (world.get::<PluginInfo>(plugin), world.get::<Asks>(plugin))
    else {
        return;
    };
    let (id, record) =
        (info.id.clone(), Record::declined(&info.version, asks));
    if let Some(mut store) = world.get_resource_mut::<Store>() {
        store.set(&id, record);
    }
}
