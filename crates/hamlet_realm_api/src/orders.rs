//! The chief's deeds on the world, carried to the core as the
//! commands a chief could type or to the realm as events. The panels
//! say each [`Decree`] and [`Tell`] in the chief's chat.

use bevy::ecs::entity::Entity;
use bevy::ecs::event::Event;
use bevy::ecs::world::World;

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

/// Gives `agent` the skill `skill` of the agent `from`, or the
/// starter of that name, and shows it arrive.
#[derive(Event, Clone, Debug)]
pub struct Give {
    pub agent: Entity,
    pub skill: String,
    pub from: Option<String>,
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

/// Asks the realm to have two agents meet, as [`Introduce`] says.
pub fn introduce(world: &mut World, a: Entity, b: Entity) {
    world.trigger(Introduce { a, b });
    world.flush();
}

/// Asks the realm to give `agent` a skill, as [`Give`] says.
pub fn give(
    world: &mut World,
    agent: Entity,
    skill: &str,
    from: Option<&str>,
) {
    world.trigger(Give {
        agent,
        skill: skill.into(),
        from: from.map(str::to_string),
    });
    world.flush();
}

/// Asks the realm to put `what` in the chief's hand.
pub fn carry(world: &mut World, what: Carry) {
    world.trigger(what);
    world.flush();
}
