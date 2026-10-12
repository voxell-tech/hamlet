//! A chief's instruction while it is carried out: the objections to
//! it, the mayor's review of it, and the end of its turn.

use bevy_ecs::prelude::*;
use hamlet_home::limits::Review;
use hamlet_rules::chief::rulings::{Choice, Hearing, Party};
use hamlet_rules::hamlet::MAYOR;

use super::decide::{Asked, Held, Parked};
use super::queue::{Kind, Waiting, raise};
use super::{Unreviewed, limits, review};
use crate::agent::Event as AgentEvent;
use crate::core::Settings;
use crate::session::Reply;

/// The instruction an objection is about.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
#[relationship(relationship_target = Objections)]
pub(super) struct About(pub(super) Entity);

/// The objections to an instruction that wait for a chief.
#[derive(Component, Default, Debug)]
#[relationship_target(relationship = About)]
pub(super) struct Objections(Vec<Entity>);

/// The objections to `instruction` that wait for a chief.
pub(super) fn objections(
    world: &World,
    instruction: Entity,
) -> Vec<Entity> {
    world
        .get::<Objections>(instruction)
        .map(|objections| objections.0.clone())
        .unwrap_or_default()
}

/// A chief's instruction while it is carried out.
#[derive(Component, Clone, Debug)]
pub(crate) struct Instruction {
    pub agent: String,
    pub chief: String,
    pub text: String,
    pub session: Entity,
    pub hearing: Hearing,
    /// The task its spend is counted under, once its turn began.
    pub task: Option<String>,
    /// The mayor has been asked about it.
    pub(super) reviewed: bool,
}

/// The instruction a session is carrying out.
#[derive(Component)]
pub(super) struct Carrying(pub(super) Entity);

/// The agents at work inside a session's running turn, besides its
/// own.
#[derive(Component, Default)]
pub(super) struct Chain(pub(super) Vec<String>);

/// Follows a session's turn: who works inside it, when the mayor
/// reviews its instruction, and what ends with it.
pub(super) fn follow(
    world: &mut World,
    session: Entity,
    by: &str,
    reply: &Reply,
) {
    match reply {
        Reply::Event(AgentEvent::Agent { name, event }) => {
            let mut chain = vec![name.clone()];
            let mut inner = &**event;
            while let AgentEvent::Agent { name, event } = inner {
                chain.push(name.clone());
                inner = event;
            }
            if let Ok(mut entity) = world.get_entity_mut(session) {
                entity.insert(Chain(chain));
            }
        }
        // The work has begun; the mayor looks at it beside the
        // agent.
        Reply::Event(AgentEvent::ModelCall(_)) => {
            let Some(instruction) =
                world.get::<Carrying>(session).map(|c| c.0)
            else {
                return;
            };
            let reviews = reviews(world);
            let Some(mut told) =
                world.get_mut::<Instruction>(instruction)
            else {
                return;
            };
            if reviews && !told.reviewed && told.agent != MAYOR {
                told.reviewed = true;
                let told = told.clone();
                review::ask(world, instruction, &told);
            }
        }
        Reply::Done(_) | Reply::Error(_) => {
            if let Ok(mut entity) = world.get_entity_mut(session) {
                entity.remove::<(Chain, Asked)>();
            }
            ended(world, session);
            if let Reply::Error(error) = reply {
                fault(world, session, by, error);
            }
        }
        _ => {}
    }
}

/// True when the mayor is asked about an instruction now.
fn reviews(world: &World) -> bool {
    if world.contains_resource::<Unreviewed>() {
        return false;
    }
    match limits(world).asking.mayor_review {
        Review::Always => true,
        Review::Never => false,
        // One local model takes turns, so there the review holds up
        // the work it is about.
        Review::Remote => world
            .resource::<Settings>()
            .config
            .default_endpoint()
            .is_some_and(|endpoint| {
                review::remote(&endpoint.base_url)
            }),
    }
}

/// The turn of `session` is over, and with it the instruction it
/// carried out.
fn ended(world: &mut World, session: Entity) {
    let Some(instruction) = world
        .get_entity_mut(session)
        .ok()
        .and_then(|mut e| e.take::<Carrying>())
        .map(|c| c.0)
    else {
        return;
    };
    over(world, instruction);
}

/// Ends `instruction`. An objection to it that nobody answered
/// stays for the chief to read.
pub(super) fn over(world: &mut World, instruction: Entity) {
    for item in objections(world, instruction) {
        let mut entity = world.entity_mut(item);
        entity.remove::<(About, Held, Parked)>();
        if let Some(mut waiting) = entity.get_mut::<Waiting>() {
            waiting.answers = vec![Choice::Dismiss];
            waiting
                .about
                .push_str("\n(The work ended before you answered.)");
        }
    }
    if let Ok(entity) = world.get_entity_mut(instruction) {
        entity.despawn();
    }
}

fn fault(world: &mut World, session: Entity, by: &str, error: &str) {
    let same = world
        .query::<&mut Waiting>()
        .iter_mut(world)
        .find(|w| {
            w.kind == Kind::Fault && w.by == by && w.about == error
        })
        .map(|mut w| w.asked += 1);
    if same.is_none() {
        raise(
            world,
            Kind::Fault,
            by,
            error.to_string(),
            Some(session),
            vec![Choice::Dismiss],
        );
    }
}

/// The mayor's objection to an instruction, from its review.
pub(super) fn mayor_objects(
    world: &mut World,
    instruction: Entity,
    told: &Instruction,
    reason: &str,
) {
    let about = format!(
        "{} told {}: {}\nThe mayor objects: {reason}",
        told.chief, told.agent, told.text
    );
    let hearing = world
        .get_mut::<Instruction>(instruction)
        .map(|mut i| i.hearing.object(Party::Mayor));
    match hearing {
        Some(Ok(())) => {
            let item = raise(
                world,
                Kind::Objection(Party::Mayor),
                MAYOR,
                about,
                Some(told.session),
                vec![Choice::GoAhead, Choice::Withdraw],
            );
            world.entity_mut(item).insert(About(instruction));
        }
        // The chief has already said go ahead.
        Some(Err(_)) => {}
        None => {
            raise(
                world,
                Kind::Objection(Party::Mayor),
                MAYOR,
                format!("{about}\n(The work had already ended.)"),
                None,
                vec![Choice::Dismiss],
            );
        }
    }
}
