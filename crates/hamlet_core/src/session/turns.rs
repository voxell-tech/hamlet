//! Telling of the turns of the agents, for whoever only watches: a
//! turn began, and it ended with these tools. Nothing here can
//! change or stop a turn.

use bevy_ecs::prelude::*;

use super::{Reply, Seat};
use crate::agent::Event as AgentEvent;

/// The most tools a turn lists. The rest are left out.
const TOOLS_MAX: usize = 32;

/// Which end of a turn.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Before,
    After,
}

/// A turn of an agent began or ended. It carries no word of what was
/// said, and no argument or output of any tool.
#[derive(Event, Clone, PartialEq, Debug)]
pub struct TurnObserved {
    pub phase: Phase,
    pub agent: String,
    pub hamlet: String,
    /// How many turns the session has had, this one included.
    pub number: u64,
    /// The names of the tools the turn used, in order. Empty before.
    pub tools: Vec<String>,
}

/// On a session: the turn it is in.
#[derive(Component, Default)]
pub(super) struct Turning {
    number: u64,
    open: bool,
    tools: Vec<String>,
}

/// A session began a turn.
pub(super) fn begin(world: &mut World, session: Entity) {
    let Ok(mut entity) = world.get_entity_mut(session) else {
        return;
    };
    let mut turning = entity.take::<Turning>().unwrap_or_default();
    turning.number += 1;
    turning.open = true;
    turning.tools.clear();
    tell(world, session, Phase::Before, &turning);
    world.entity_mut(session).insert(turning);
}

/// Notes what a session's turn did, and tells when it ends.
pub(super) fn note(
    world: &mut World,
    session: Entity,
    reply: &Reply,
) {
    let Some(mut turning) = world
        .get_entity_mut(session)
        .ok()
        .and_then(|mut entity| entity.take::<Turning>())
    else {
        return;
    };
    match reply {
        Reply::Event(AgentEvent::Tool { name, .. })
            if turning.tools.len() < TOOLS_MAX =>
        {
            turning.tools.push(name.clone());
        }
        Reply::Done(_) | Reply::Error(_) if turning.open => {
            turning.open = false;
            tell(world, session, Phase::After, &turning);
        }
        _ => {}
    }
    world.entity_mut(session).insert(turning);
}

fn tell(
    world: &mut World,
    session: Entity,
    phase: Phase,
    turning: &Turning,
) {
    let Some(seat) = world.get::<Seat>(session) else {
        return;
    };
    let observed = TurnObserved {
        phase,
        agent: seat.name.clone(),
        hamlet: seat.hamlet.clone(),
        number: turning.number,
        tools: turning.tools.clone(),
    };
    world.trigger(observed);
}
