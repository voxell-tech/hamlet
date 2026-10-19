//! The chief's deeds on the world, carried to the core as the
//! commands a chief could type or to the realm as events. The panels
//! say each [`Decree`] and [`Tell`] in the chief's chat.

use bevy::ecs::entity::Entity;
use bevy::ecs::event::Event;
use bevy::ecs::name::Name;
use bevy::ecs::world::World;
use hamlet_core::agents::{self, LivesIn, Settlement};
use hamlet_rules::gifts::Offer;
use hamlet_rules::hamlet::{HOME, Hamlet};

/// A chief's command for the hamlet `hamlet`, as the chief would
/// type it.
#[derive(Event, Clone, Debug, PartialEq)]
pub struct Decree {
    pub hamlet: String,
    pub text: String,
}

/// Words from the chief to `agent`, in its own chat.
#[derive(Event, Clone, Debug, PartialEq)]
pub struct Tell {
    pub agent: Entity,
    pub text: String,
}

/// Has two agents greet each other and builds a road between them.
#[derive(Event, Clone, Copy, Debug)]
pub struct Introduce {
    pub a: Entity,
    pub b: Entity,
}

/// Gives `agent` the skill of `offer`, and shows it arrive.
#[derive(Event, Clone, Debug)]
pub struct Give {
    pub agent: Entity,
    pub offer: Offer,
}

/// Takes something of the world in hand, to be set down with a click
/// on the ground.
#[derive(Event, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Carry {
    /// The town of this hamlet.
    Hamlet(Entity),
    Library,
    /// The Hearth of the chief this window belongs to.
    Hearth,
}

/// The chief pressed the clockwork of `hamlet`.
#[derive(Event, Clone, Copy, Debug)]
pub struct Switched {
    pub hamlet: Entity,
}

fn name(world: &World, agent: Entity) -> Option<String> {
    world
        .get::<Name>(agent)
        .map(|name| name.as_str().to_string())
}

/// The record of the hamlet `agent` lives in.
fn record(world: &World, agent: Entity) -> Option<Hamlet> {
    let lives = world.get::<LivesIn>(agent)?;
    Some(world.get::<Settlement>(lives.0)?.record())
}

/// Gives a chief's command to the hamlet of `agent`, carried out by
/// the time this returns.
pub fn decree(world: &mut World, agent: Entity, text: String) {
    let hamlet =
        agents::hamlet_of(world, agent).unwrap_or(HOME).to_string();
    world.trigger(Decree { hamlet, text });
    world.flush();
}

/// Brings back `agent`, which was removed or merged away.
pub fn restore(world: &mut World, agent: Entity) {
    let text = format!("/restore {}", agents::name_of(world, agent));
    decree(world, agent, text);
}

/// Takes up the road between two agents at the chief's word.
pub fn part(world: &mut World, a: Entity, b: Entity) {
    if let (Some(a_name), Some(b_name)) =
        (name(world, a), name(world, b))
    {
        decree(world, a, format!("/unroad {a_name} {b_name}"));
    }
}

/// The skills `agent` could be given.
pub fn offers(world: &World, agent: Entity) -> Vec<Offer> {
    name(world, agent)
        .zip(record(world, agent))
        .map(|(name, hamlet)| hamlet.offers(&name))
        .unwrap_or_default()
}

/// Asks the realm to have two agents meet, as [`Introduce`] says.
pub fn introduce(world: &mut World, a: Entity, b: Entity) {
    world.trigger(Introduce { a, b });
    world.flush();
}

/// Asks the realm to give `agent` a skill, as [`Give`] says.
pub fn give(world: &mut World, agent: Entity, offer: &Offer) {
    world.trigger(Give {
        agent,
        offer: offer.clone(),
    });
    world.flush();
}

/// Asks the realm to put `what` in the chief's hand.
pub fn carry(world: &mut World, what: Carry) {
    world.trigger(what);
    world.flush();
}
