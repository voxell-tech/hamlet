//! The chief choosing for one capability of one plugin.

use bevy::ecs::entity::Entity;
use bevy::ecs::event::Event;
use bevy::ecs::observer::On;
use bevy::ecs::system::{Commands, Query};
use bevy::ecs::world::World;

use super::ask::Asks;
use super::choice::Choice;
use super::granted::Choices;
use super::name::Capability;
use super::record::{Installation, Record};
use super::store::Store;
use crate::plugin::PluginInfo;

/// The chief chooses for a capability of the plugin of this id. The
/// plugin's next call sees it. A capability the manifest does not
/// name cannot be chosen for.
#[derive(Event, Clone, PartialEq, Debug)]
pub struct Decide {
    pub plugin: String,
    pub capability: Capability,
    /// The plugin, or collection, that a scoped capability is chosen
    /// for.
    pub target: Option<String>,
    pub choice: Choice,
    /// Lets an [`Choice::AskNextTime`] through for the rest of this
    /// run.
    pub this_run: bool,
}

/// The chief's choices for every target of the scoped `capability`
/// of the plugin of this id are dropped, as when the one folder a
/// plugin works in is chosen again.
#[derive(Event, Clone, PartialEq, Debug)]
pub struct Forget {
    pub plugin: String,
    pub capability: Capability,
}

pub(super) fn decide(
    decide: On<Decide>,
    plugins: Query<(Entity, &PluginInfo, &Asks)>,
    mut commands: Commands,
) {
    let decide = decide.event().clone();
    let Some(entity) =
        named(&plugins, &decide.plugin, decide.capability)
    else {
        return;
    };
    // Applied in turn, each on the choices the one before left.
    commands.queue(move |world: &mut World| {
        apply(world, entity, &decide.plugin, |choices| match &decide
            .target
        {
            Some(target) => choices.decided_for(
                decide.capability,
                target,
                decide.choice,
                decide.this_run,
            ),
            None => choices.decided(
                decide.capability,
                decide.choice,
                decide.this_run,
            ),
        })
    });
}

pub(super) fn forget(
    forget: On<Forget>,
    plugins: Query<(Entity, &PluginInfo, &Asks)>,
    mut commands: Commands,
) {
    let forget = forget.event().clone();
    let Some(entity) =
        named(&plugins, &forget.plugin, forget.capability)
    else {
        return;
    };
    commands.queue(move |world: &mut World| {
        apply(world, entity, &forget.plugin, |choices| {
            choices.forgetting(forget.capability)
        })
    });
}

/// The plugin of this id, if its manifest names `capability`.
fn named(
    plugins: &Query<(Entity, &PluginInfo, &Asks)>,
    plugin: &str,
    capability: Capability,
) -> Option<Entity> {
    plugins
        .iter()
        .find(|(_, info, asks)| {
            info.id == plugin && asks.names(capability)
        })
        .map(|(entity, ..)| entity)
}

/// Puts the choices `change` makes of the plugin's in place, and in
/// the record of an installed plugin.
fn apply(
    world: &mut World,
    plugin: Entity,
    id: &str,
    change: impl FnOnce(Choices) -> Choices,
) {
    let Some(choices) = world.get::<Choices>(plugin) else {
        return;
    };
    let asks = world.get::<Asks>(plugin).cloned().unwrap_or_default();
    let choices = change(choices.clone());
    if let Some(mut store) = world.get_resource_mut::<Store>() {
        let installed = store
            .get(id)
            .filter(|record| record.state == Installation::Installed)
            .map(|record| record.version.clone());
        if let Some(version) = installed {
            store.set(
                id,
                Record::installed(&version, &choices).scoped(&asks),
            );
        }
    }
    if let Ok(mut plugin) = world.get_entity_mut(plugin) {
        plugin.insert(choices);
    }
}
