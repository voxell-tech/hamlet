//! The ends that come without a chief's answer: a turn the chief
//! stops, a conversation taken back, an agent that leaves, and the
//! app quitting.

use std::time::Duration;

use bevy_ecs::prelude::*;
use hamlet_model::log;
use hamlet_rules::chief::rulings::Choice;

use crate::agents::{Citizen, Left};
use crate::oversight::grants::Grants;
use crate::oversight::instruction::Chain;
use crate::oversight::ledger::{self, Ledger};
use crate::oversight::queue::{Kind, Needs, Waiting, close, raise};
use crate::session::{Seat, SessionKey, Status, Stop};

/// A chief stops what an agent is doing now, or everything (rule
/// one).
#[derive(Event, Clone, Debug)]
pub struct Veto {
    /// `None` stops every agent.
    pub agent: Option<String>,
    pub chief: String,
}

/// Stops the turns a veto names and drops what they were waiting
/// for.
pub(super) fn stop(world: &mut World, veto: Veto) {
    let sessions =
        world
            .query::<(
                Entity,
                &SessionKey,
                &Status,
                Option<&Chain>,
                Option<&Seat>,
            )>()
            .iter(world)
            .filter(|(_, key, status, chain, seat)| {
                status.working()
                    && veto.agent.as_deref().is_none_or(|name| {
                        // An agent is named by the office while it is
                        // mayor, and by its own name otherwise. A
                        // chief says either.
                        key.address() == name
                            || seat.is_some_and(|s| s.name == name)
                            || chain.is_some_and(|c| {
                                c.0.iter().any(|n| n == name)
                            })
                    })
            })
            .map(|(e, ..)| e)
            .collect::<Vec<_>>();
    for session in sessions {
        world.trigger(Stop {
            session,
            reason: format!("stopped by {}", veto.chief),
        });
        let waiting = world
            .query::<(Entity, &Waiting)>()
            .iter(world)
            .filter(|(_, w)| {
                w.session == Some(session)
                    && !matches!(w.kind, Kind::Fault | Kind::Lapsed)
            })
            .map(|(e, _)| e)
            .collect::<Vec<_>>();
        for item in waiting {
            close(world, item, "stopped", "", Some(&veto.chief));
        }
    }
}

/// Drops what the conversation `session` raised since `at`, in
/// milliseconds since 1970: it is about a part of it that a rewind
/// took out.
pub(super) fn forget(world: &mut World, session: Entity, at: u64) {
    let age =
        Duration::from_millis(log::now_millis().saturating_sub(at));
    let waiting = world
        .query::<(Entity, &Waiting)>()
        .iter(world)
        .filter(|(_, w)| {
            w.session == Some(session)
                && w.kind != Kind::Lapsed
                && w.waited() <= age
        })
        .map(|(e, _)| e)
        .collect::<Vec<_>>();
    for item in waiting {
        close(world, item, "rewound", "", None);
    }
}

/// Drops what an agent that left the hamlet was waiting for, and
/// what it was granted. One that comes back asks again.
pub(super) fn lapse(world: &mut World, agent: Entity) {
    let waiting = world
        .get::<Needs>(agent)
        .map(|needs| needs.iter().collect::<Vec<_>>())
        .unwrap_or_default();
    for item in waiting {
        close(
            world,
            item,
            "lapsed",
            "the agent left the hamlet",
            None,
        );
    }
    let Some(number) = world.get::<Citizen>(agent).map(|c| c.0)
    else {
        return;
    };
    let mut grants = world.resource_mut::<Grants>();
    let held = grants
        .held()
        .iter()
        .filter(|g| g.agent == number)
        .map(|g| g.permission)
        .collect::<Vec<_>>();
    for permission in held {
        let _ = grants.revoke(number, permission, "hamlet");
    }
}

/// Lapses what agents that left while the app was closed still
/// held.
pub(super) fn departed(world: &mut World) {
    let gone = world
        .query_filtered::<Entity, With<Left>>()
        .iter(world)
        .collect::<Vec<_>>();
    for agent in gone {
        lapse(world, agent);
    }
}

/// Raises again what was waiting when the app last quit. None of it
/// was carried out.
pub(super) fn recall(world: &mut World) {
    let ledger = world.resource::<Ledger>();
    let open = ledger::unsettled(&ledger.events());
    for old in &open {
        let _ =
            ledger.settled(&old.id, "lapsed", "hamlet quit", None);
    }
    for old in open {
        let (kind, about) = match old.kind.as_str() {
            "fault" => (Kind::Fault, old.about),
            // It says so already.
            "lapsed" => (Kind::Lapsed, old.about),
            kind => (
                Kind::Lapsed,
                format!(
                    "{}\n(This {kind} was waiting for you when \
                     Hamlet last quit. It was not carried out.)",
                    old.about
                ),
            ),
        };
        raise(
            world,
            kind,
            &old.by,
            about,
            None,
            vec![Choice::Dismiss],
        );
    }
}
