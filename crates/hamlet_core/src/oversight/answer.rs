//! A chief's answer to something that waits, and what it lets go.

use bevy_ecs::prelude::*;
use hamlet_rules::chief::rulings::Choice;

use super::danger::{Danger, Refused};
use super::decide::{Asked, Held, Parked, Trusted, standing};
use super::grants::{self, Grants};
use super::instruction::{About, Instruction, objections};
use super::limits;
use super::queue::{Kind, Waiting, close, raiser};
use crate::agent::Ruling;
use crate::session::{self, Began, Stop};
use crate::tools::Leave;

/// A chief's answer to something that waits.
#[derive(EntityEvent, Clone, Debug)]
pub struct Answer {
    pub entity: Entity,
    pub choice: Choice,
    /// The reason or the reply.
    pub text: String,
    pub chief: String,
}

/// Sends `ruling` to every call `entity` holds.
fn release(world: &mut World, entity: Entity, ruling: &Ruling) {
    let held = world
        .get_mut::<Held>(entity)
        .map(|mut h| std::mem::take(&mut h.0))
        .unwrap_or_default();
    for answer in held {
        let _ = answer.send(ruling.clone());
    }
}

/// Applies a chief's answer. One that the thing does not offer is
/// ignored.
pub(super) fn settle(world: &mut World, answer: Answer) {
    let Answer {
        entity,
        choice,
        text,
        chief,
    } = answer;
    let Some(waiting) = world.get::<Waiting>(entity).cloned() else {
        return;
    };
    if !waiting.answers.contains(&choice) {
        return;
    }
    let text = text.trim().to_string();
    let with = |lead: &str| {
        if text.is_empty() {
            format!("{lead}.")
        } else {
            format!("{lead}: {text}")
        }
    };
    match (waiting.kind, choice) {
        (Kind::Approval(danger), Choice::Allow | Choice::Always) => {
            if let (Choice::Always, Some(permission)) =
                (choice, grants::lifting(danger))
            {
                let given =
                    raiser(world, entity).is_some_and(|agent| {
                        world
                            .resource_mut::<Grants>()
                            .give(
                                agent,
                                &waiting.by,
                                permission,
                                &chief,
                            )
                            .is_ok()
                    });
                if !given {
                    // Without the record there is no grant: the
                    // chief must answer again.
                    return;
                }
            }
            if choice == Choice::Always && danger.for_session() {
                let began = waiting
                    .session
                    .and_then(|s| world.get::<Began>(s).copied());
                if let (Some(session), Some(began)) =
                    (waiting.session, began)
                {
                    world
                        .entity_mut(session)
                        .entry::<Trusted>()
                        .or_default()
                        .into_mut()
                        .0
                        .push((waiting.by.clone(), danger, began));
                }
            }
            // An agent that asked only because its hamlet has it
            // ask keeps the network it already had.
            let had_net = danger.for_session()
                && waiting.session.is_some_and(|session| {
                    let limits = limits(world);
                    standing(world, &limits, &waiting.by, session).net
                });
            let leave = Leave {
                net: danger == Danger::Network || had_net,
                reserved: danger == Danger::Reserved,
            };
            release(world, entity, &Ruling::Go(leave));
        }
        (Kind::Approval(danger), Choice::Deny) => {
            let reason = if text.is_empty() {
                "no reason given".to_string()
            } else {
                text.clone()
            };
            if let Some(mut asked) = waiting
                .session
                .and_then(|s| world.get_mut::<Asked>(s))
            {
                asked.strikes += 1;
                asked.refused.push((
                    waiting.by.clone(),
                    Refused::of(danger, &waiting.about),
                    reason.clone(),
                ));
            }
            release(
                world,
                entity,
                &Ruling::Deny(format!(
                    "the chief refused it: {reason}"
                )),
            );
        }
        (Kind::Objection(_), Choice::GoAhead | Choice::Withdraw) => {
            return answer_objection(
                world,
                entity,
                choice,
                &with(match choice {
                    Choice::GoAhead => "The chief says go ahead",
                    _ => "the chief withdrew the instruction",
                }),
                &text,
                &chief,
            );
        }
        (Kind::Question, Choice::Reply) => release(
            world,
            entity,
            &Ruling::Say(format!("The chief answers: {text}")),
        ),
        (Kind::Question, Choice::Dismiss) => release(
            world,
            entity,
            &Ruling::Say(
                "The chief chose not to answer. Go on with your \
                 best judgement, and say what you assumed."
                    .into(),
            ),
        ),
        _ => {}
    }
    close(world, entity, choice.word(), &text, Some(&chief));
}

/// Lets a held instruction go on, or takes it back. Every objection
/// to the instruction ends with it.
fn answer_objection(
    world: &mut World,
    entity: Entity,
    choice: Choice,
    said: &str,
    text: &str,
    chief: &str,
) {
    let instruction = world.get::<About>(entity).map(|a| a.0);
    let items = match instruction {
        Some(instruction) => objections(world, instruction),
        None => vec![entity],
    };
    let told = instruction
        .and_then(|i| world.get_mut::<Instruction>(i))
        .map(|mut told| {
            if choice == Choice::GoAhead {
                told.hearing.go_ahead();
            }
            told.session
        });
    if let (Choice::Withdraw, Some(session)) = (choice, told) {
        // Before the held calls are let go, so the turn ends
        // without taking another step.
        world.trigger(Stop {
            session,
            reason: said.to_string(),
        });
    }
    for item in items {
        let parked = world
            .get_mut::<Parked>(item)
            .map(|mut p| std::mem::take(&mut p.0))
            .unwrap_or_default();
        if choice == Choice::GoAhead {
            release(
                world,
                item,
                &Ruling::Say(format!(
                    "{said} Carry out the instruction now. You may \
                     not object to it again."
                )),
            );
            for call in parked {
                session::rule(world, call);
            }
        }
        close(world, item, choice.word(), text, Some(chief));
    }
}
