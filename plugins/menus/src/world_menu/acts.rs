//! What each item of the menu of a thing in the world does to the
//! thing, and when an item is there.

use bevy::ecs::entity::Entity;
use bevy::ecs::name::Name;
use bevy::ecs::system::Commands;
use bevy::ecs::world::World;
use bevy::math::Vec2;
use hamlet_agents_api::agents::{
    self, Busy, Left, Mayor, Settlement,
};
use hamlet_agents_api::hamlet::HOME;
use hamlet_chat_api::deed;
use hamlet_realm_api::orders::{self, Carry};
use hamlet_realm_api::pick::{Thing, focus_on, select};
use hamlet_sdk::command::{Subject, kind};

use super::deed::Deed;
use super::subject::{entity, thing};
use super::{Open, pointer};
use crate::chat;
use crate::me::{talk_to, talk_to_mayor};

/// Queues `act` on `subject`, for a command that needs the whole
/// world.
pub(super) fn queue(
    commands: &mut Commands,
    subject: &Subject,
    act: fn(&mut World, &Subject),
) {
    let subject = subject.clone();
    commands.queue(move |world: &mut World| act(world, &subject));
}

/// Where the menu that is open was asked for, or the pointer where
/// none is.
fn at(world: &mut World) -> Vec2 {
    match world.get_resource::<Open>() {
        Some(Open::Menu(_, at) | Open::Ground(at)) => *at,
        _ => pointer(world),
    }
}

/// Asks before `deed`, where the menu was.
fn ask(world: &mut World, deed: Deed) {
    let at = at(world);
    *world.resource_mut::<Open>() = Open::Question(deed, at);
}

pub(super) fn talk(world: &mut World, subject: &Subject) {
    match entity(subject) {
        _ if subject.kind == kind::MAYOR => talk_to_mayor(world),
        Some(agent) => talk_to(world, agent),
        None => {}
    }
}

pub(super) fn focus(world: &mut World, subject: &Subject) {
    if let Some(thing) = thing(subject) {
        focus_on(world, thing, false);
    }
}

pub(super) fn follow(world: &mut World, subject: &Subject) {
    if let Some(thing) = thing(subject) {
        focus_on(world, thing, true);
    }
}

pub(super) fn halt(world: &mut World, subject: &Subject) {
    let agent = match subject.kind == kind::MAYOR {
        true => agents::mayor(world),
        false => entity(subject),
    };
    if let Some(agent) = agent {
        deed::stop(world, agent);
    }
}

pub(super) fn undo(world: &mut World, subject: &Subject) {
    let text = "/undo".to_string();
    match (subject.kind == kind::MAYOR, entity(subject)) {
        (true, _) => chat::decree(world, HOME, text),
        (_, Some(agent)) => orders::decree(world, agent, text),
        _ => {}
    }
}

pub(super) fn make_mayor(world: &mut World, subject: &Subject) {
    if let Some(agent) = entity(subject) {
        ask(world, Deed::Mayor(agent));
    }
}

pub(super) fn remove(world: &mut World, subject: &Subject) {
    if let Some(agent) = entity(subject) {
        ask(world, Deed::Remove(agent));
    }
}

pub(super) fn give_skill(world: &mut World, subject: &Subject) {
    if let Some(agent) = entity(subject) {
        let at = at(world);
        *world.resource_mut::<Open>() = Open::Skills(agent, at);
    }
}

pub(super) fn bring_back(world: &mut World, subject: &Subject) {
    if let Some(agent) = entity(subject) {
        orders::restore(world, agent);
    }
}

pub(super) fn change_look(world: &mut World, subject: &Subject) {
    if let Some(chief) = entity(subject) {
        select(world, Some(Thing::ChiefPlace(chief)));
    }
}

pub(super) fn go_to_hearth(world: &mut World, subject: &Subject) {
    if let Some(chief) = entity(subject) {
        focus_on(world, Thing::ChiefPlace(chief), false);
    }
}

pub(super) fn move_hearth(world: &mut World, _: &Subject) {
    orders::carry(world, Carry::Hearth);
}

pub(super) fn carry(world: &mut World, subject: &Subject) {
    match (subject.kind.as_str(), entity(subject)) {
        (kind::HAMLET, Some(hamlet)) => {
            orders::carry(world, Carry::Hamlet(hamlet))
        }
        (kind::LIBRARY, _) => orders::carry(world, Carry::Library),
        _ => {}
    }
}

pub(super) fn close_down(world: &mut World, subject: &Subject) {
    let id = entity(subject)
        .and_then(|hamlet| world.get::<Settlement>(hamlet))
        .map(|settlement| settlement.name.clone());
    if let Some(id) = id {
        ask(world, Deed::Close(id));
    }
}

pub(super) fn found(world: &mut World, _: &Subject) {
    let at = at(world);
    *world.resource_mut::<Open>() = Open::Founding(at);
}

/// Whether the agent has a turn running.
pub(super) fn busy(world: &World, subject: &Subject) -> bool {
    entity(subject)
        .is_some_and(|agent| world.get::<Busy>(agent).is_some())
}

/// Whether the agent is neither the mayor nor gone, so that the
/// deeds that change who it is are open to it.
pub(super) fn plain(world: &World, subject: &Subject) -> bool {
    entity(subject).is_some_and(|agent| plain_agent(world, agent))
}

pub(super) fn plain_agent(world: &World, agent: Entity) -> bool {
    world.get::<Mayor>(agent).is_none()
        && world.get::<Left>(agent).is_none()
        && world.get::<Name>(agent).is_some()
}

/// Whether the agent could be given a skill.
pub(super) fn can_learn(world: &World, subject: &Subject) -> bool {
    plain(world, subject)
        && entity(subject).is_some_and(|agent| {
            !orders::offers(world, agent).is_empty()
        })
}

/// Whether the hamlet may be closed down: the first one stays.
pub(super) fn closable(world: &World, subject: &Subject) -> bool {
    entity(subject)
        .and_then(|hamlet| world.get::<Settlement>(hamlet))
        .is_some_and(|settlement| {
            !settlement.name.is_empty() && settlement.name != HOME
        })
}
