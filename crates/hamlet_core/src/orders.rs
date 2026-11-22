//! The chief's commands to the hamlets, as the chief would type them,
//! and the world's actions a part asks for.

use bevy::ecs::entity::Entity;
use bevy::ecs::event::Event;
use bevy::ecs::name::Name;
use bevy::ecs::world::World;
use hamlet_client::realm::{self as agents, Actions};

/// A chief's command for the hamlet `hamlet`, or for the Hand when
/// there is none, as the chief would type it.
#[derive(Event, Clone, Debug, PartialEq)]
pub struct Decree {
    pub hamlet: Option<String>,
    pub text: String,
}

/// Words from the chief to `agent`, in its own chat.
#[derive(Event, Clone, Debug, PartialEq)]
pub struct Tell {
    pub agent: Entity,
    pub text: String,
}

/// Has two agents greet each other in the world and opens a channel
/// between them.
#[derive(Event, Clone, Copy, Debug, PartialEq)]
pub struct Introduce(pub Entity, pub Entity);

/// Takes the town of a hamlet in hand, to be set down with a click on
/// the ground.
#[derive(Event, Clone, Copy, Debug, PartialEq)]
pub struct Carry(pub Entity);

/// Takes the house of a profile in hand, to be set down with a click
/// on the ground.
#[derive(Event, Clone, Copy, Debug, PartialEq)]
pub struct CarryHouse(pub Entity);

fn name(world: &World, agent: Entity) -> Option<String> {
    world
        .get::<Name>(agent)
        .map(|name| name.as_str().to_string())
}

/// A chief's command sent to the lead of `agent`'s hamlet, or to the
/// Hand when it has none.
pub fn decree(world: &mut World, agent: Entity, text: String) {
    let hamlet = agents::hamlet_of(world, agent).map(str::to_owned);
    world.trigger(Decree { hamlet, text });
    world.flush();
}

/// Opens a channel between two agents at the chief's word.
pub fn channel(world: &mut World, a: Entity, b: Entity) {
    if !Actions::is_allowed(world, "channel") {
        return;
    }
    if let (Some(a_name), Some(b_name)) =
        (name(world, a), name(world, b))
    {
        decree(world, a, format!("/channel {a_name} {b_name}"));
    }
}

/// Closes the channel between two agents at the chief's word.
pub fn part(world: &mut World, a: Entity, b: Entity) {
    if !Actions::is_allowed(world, "unchannel") {
        return;
    }
    if let (Some(a_name), Some(b_name)) =
        (name(world, a), name(world, b))
    {
        decree(world, a, format!("/unchannel {a_name} {b_name}"));
    }
}
