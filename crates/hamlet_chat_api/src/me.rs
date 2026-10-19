//! The agent the chat is with, and the ways to point it at another.

use bevy::ecs::change_detection::DetectChangesMut;
use bevy::ecs::entity::Entity;
use bevy::ecs::schedule::SystemSet;
use bevy::ecs::system::{Res, ResMut};
use bevy::ecs::world::World;
use hamlet_agents_api::convo::AgentSelected;
pub use hamlet_agents_api::convo::{Me, address};
use hamlet_agents_api::{agents, session};
use hamlet_realm_api::pick::{Selected, Thing, select};
use hamlet_sdk::dock::show_tab;

/// The system that keeps [`Me`] true to the roster.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct Reseated;

/// The system that points the chat at the agent the world selects.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct Followed;

/// Keeps [`AgentSelected`] true to the world's selection.
pub fn mark_selected(
    selected: Res<Selected>,
    mut marked: ResMut<AgentSelected>,
) {
    marked.set_if_neq(AgentSelected(matches!(
        selected.0,
        Some(
            Thing::Keeper
                | Thing::Cottage
                | Thing::Resident(_)
                | Thing::ResidentHouse(_)
        )
    )));
}

/// Makes `agent` who the chat is with. False when it is not there.
pub fn seat(world: &mut World, agent: Entity) -> bool {
    let Some((key, with)) = address(world, agent) else {
        return false;
    };
    let session = session::ensure(world, &key);
    // Its letters to and from other agents are part of its chat.
    session::gather(world, agent);
    let mut me = world.resource_mut::<Me>();
    me.session = session;
    me.key = key;
    me.agent = agent;
    me.with = with;
    true
}

/// Points the chat at one of the hamlet's agents, and selects it in
/// the world.
pub fn talk_to(world: &mut World, agent: Entity) {
    if !seat(world, agent) {
        return;
    }
    let mayor = agents::mayor(world) == Some(agent);
    let selected =
        world.get_resource::<Selected>().map(|selected| selected.0);
    let same = match selected {
        Some(Some(Thing::Keeper | Thing::Cottage)) => mayor,
        Some(Some(
            Thing::Resident(of) | Thing::ResidentHouse(of),
        )) => of == agent,
        Some(_) => false,
        // Nothing can be selected where there is no world.
        None => true,
    };
    if !same {
        let thing = if mayor {
            Thing::Keeper
        } else {
            Thing::Resident(agent)
        };
        select(world, Some(thing));
    }
    show_tab(world, "agent");
}

/// Points the chat back at the mayor.
pub fn talk_to_mayor(world: &mut World) {
    if let Some(mayor) = agents::mayor(world) {
        talk_to(world, mayor);
    }
}

/// Points the chat at whoever is mayor of `hamlet`.
pub fn talk_to_mayor_of(world: &mut World, hamlet: &str) {
    if let Some(mayor) = agents::mayor_of(world, hamlet) {
        talk_to(world, mayor);
    }
}
