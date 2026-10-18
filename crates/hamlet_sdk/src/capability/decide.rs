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

pub(super) fn decide(
    decide: On<Decide>,
    plugins: Query<(Entity, &PluginInfo, &Asks)>,
    mut commands: Commands,
) {
    let decide = decide.event().clone();
    let Some(entity) = plugins
        .iter()
        .find(|(_, info, asks)| {
            info.id == decide.plugin && asks.names(decide.capability)
        })
        .map(|(entity, ..)| entity)
    else {
        return;
    };
    // Applied in turn, each on the choices the one before left.
    commands
        .queue(move |world: &mut World| apply(world, entity, decide));
}

fn apply(world: &mut World, plugin: Entity, decide: Decide) {
    let Some(choices) = world.get::<Choices>(plugin) else {
        return;
    };
    let asks = world.get::<Asks>(plugin).cloned().unwrap_or_default();
    let choices = match &decide.target {
        Some(target) => choices.clone().decided_for(
            decide.capability,
            target,
            decide.choice,
            decide.this_run,
        ),
        None => choices.clone().decided(
            decide.capability,
            decide.choice,
            decide.this_run,
        ),
    };
    if let Some(mut store) = world.get_resource_mut::<Store>() {
        let installed = store
            .get(&decide.plugin)
            .filter(|record| record.state == Installation::Installed)
            .map(|record| record.version.clone());
        if let Some(version) = installed {
            store.set(
                &decide.plugin,
                Record::installed(&version, &choices).scoped(&asks),
            );
        }
    }
    if let Ok(mut plugin) = world.get_entity_mut(plugin) {
        plugin.insert(choices);
    }
}
