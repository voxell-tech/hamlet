//! The ruling on a tool call: it runs, it is blocked, or it is held
//! for a chief.

use bevy_ecs::prelude::*;
use hamlet_home::limits::{Limits, Permission};
use hamlet_rules::chief::asking as questions;
use hamlet_rules::chief::rulings::{self, Choice, Party};
use hamlet_rules::hamlet::{Hamlet, MAYOR};
use serde_json::Value;
use tokio::sync::oneshot;

use super::danger::{self, Danger, Refused, Standing, Verdict};
use super::grants::{self, Grants};
use super::instruction::{About, Carrying, Instruction, objections};
use super::queue::{Asks, Kind, Waiting, raise};
use super::{Rules, limits};
use crate::agent::Ruling;
use crate::agents::{self, Citizen};
use crate::core::{HomeDir, Sandboxing, Settings};
use crate::session::{Began, ToolStarting};
use crate::tools::Reach;

/// The tool calls an answer to a [`Waiting`] releases.
#[derive(Component, Default)]
pub(super) struct Held(pub(super) Vec<oneshot::Sender<Ruling>>);

/// Tool calls of a held instruction, to be judged once it is let
/// go.
#[derive(Component, Default)]
pub(super) struct Parked(pub(super) Vec<ToolStarting>);

/// The refusals a session has had in its running turn, and the count
/// of its questions.
#[derive(Component, Default)]
pub(super) struct Asked {
    pub(super) refused: Vec<(String, Refused, String)>,
    pub(super) strikes: u32,
    questions: u32,
}

/// The edits or commands a chief allowed an agent for the rest of
/// a session, with when that session began.
#[derive(Component, Default)]
pub(super) struct Trusted(pub(super) Vec<(String, Danger, Began)>);

pub(super) fn standing(
    world: &mut World,
    limits: &Limits,
    who: &str,
    session: Entity,
) -> Standing {
    let entity = agents::named(world, who);
    let agent = entity
        .and_then(|agent| world.get::<Citizen>(agent))
        .map(|citizen| citizen.0);
    let manual = entity
        .and_then(|agent| agents::hamlet_of(world, agent))
        .is_some_and(|place| {
            let home = &world.resource::<HomeDir>().0.root;
            Hamlet::at(home, place).work().manual()
        });
    // A leave given in a session that has since been put away does
    // not hold in the next.
    let began = world.get::<Began>(session).copied();
    let asks = |danger: Danger| {
        manual
            && !world.get::<Trusted>(session).is_some_and(|trusted| {
                trusted.0.iter().any(|(name, kind, since)| {
                    name == who
                        && *kind == danger
                        && Some(*since) == began
                })
            })
    };
    let (asks_edits, asks_commands) =
        (asks(Danger::Edit), asks(Danger::Command));
    let grants = world.resource::<Grants>();
    let holds = |permission| {
        grants::holds(
            &limits.permissions,
            grants,
            who,
            agent,
            permission,
        )
    };
    Standing {
        confined: !world.resource::<Sandboxing>().0.degraded(),
        net: world.resource::<Settings>().config.allow_net
            || holds(Permission::Network),
        unconfined: holds(Permission::Unconfined),
        asks_edits,
        asks_commands,
    }
}

/// An edit as a chief reads it: the file, then what changes.
fn edit_as_read(call: &ToolStarting) -> String {
    /// The most of either side shown, in characters.
    const SHOWN: usize = 2000;
    let part = |key: &str| {
        let text = serde_json::from_str::<Value>(&call.args)
            .ok()
            .and_then(|v| v[key].as_str().map(str::to_string))
            .unwrap_or_default();
        match text.chars().count() {
            n if n > SHOWN => format!(
                "{}\n[{} more characters]",
                text.chars().take(SHOWN).collect::<String>(),
                n - SHOWN
            ),
            _ => text,
        }
    };
    let (path, old, new) = (part("path"), part("old"), part("new"));
    if old.is_empty() {
        format!("{path}\nA new file:\n{new}")
    } else {
        format!("{path}\nReplace:\n{old}\nWith:\n{new}")
    }
}

fn argument(call: &ToolStarting, key: &str) -> String {
    serde_json::from_str::<Value>(&call.args)
        .ok()
        .and_then(|v| v[key].as_str().map(|s| s.trim().to_string()))
        .unwrap_or_default()
}

/// Rules on a tool call: lets it run, blocks it, or holds it for a
/// chief.
pub(super) fn decide(world: &mut World, mut call: ToolStarting) {
    let Some(answer) = call.answer.take() else {
        return;
    };
    let deny = |answer: oneshot::Sender<Ruling>, why: String| {
        let _ = answer.send(Ruling::Deny(why));
    };
    let instruction =
        world.get::<Carrying>(call.session).map(|c| c.0);

    if call.name == rulings::OBJECT {
        let reason = argument(&call, "reason");
        let told = instruction
            .and_then(|i| world.get_mut::<Instruction>(i))
            .filter(|i| i.agent == call.who);
        let (Some(mut told), Some(instruction)) = (told, instruction)
        else {
            return deny(
                answer,
                "only the agent a chief has just instructed may \
                 object"
                    .into(),
            );
        };
        if reason.is_empty() {
            return deny(answer, "say why in `reason`".into());
        }
        if let Err(why) = told.hearing.object(Party::Agent) {
            return deny(answer, why.into());
        }
        let about = format!(
            "{} told {}: {}\n{} objects: {reason}",
            told.chief, told.agent, told.text, told.agent
        );
        let item = raise(
            world,
            Kind::Objection(Party::Agent),
            &call.who,
            about,
            Some(call.session),
            vec![Choice::GoAhead, Choice::Withdraw],
        );
        world
            .entity_mut(item)
            .insert((Held(vec![answer]), About(instruction)));
        return;
    }

    // An instruction someone objected to does no work until the
    // chief replies.
    let held = instruction.filter(|i| {
        world
            .get::<Instruction>(*i)
            .is_some_and(|i| i.hearing.held())
    });
    if let Some(instruction) = held {
        let item = objections(world, instruction).first().copied();
        if let Some(item) = item {
            call.answer = Some(answer);
            world
                .entity_mut(item)
                .entry::<Parked>()
                .or_default()
                .into_mut()
                .0
                .push(call);
            return;
        }
    }

    // A call of a session that is gone is dropped, which blocks it.
    let Ok(mut session) = world.get_entity_mut(call.session) else {
        return;
    };
    if !session.contains::<Asked>() {
        session.insert(Asked::default());
    }
    let limits = limits(world);
    let asking = limits.asking;

    if call.name == rulings::ASK_CHIEF {
        let asked = match questions::read(&call.args) {
            Ok(asked) => asked,
            Err(why) => return deny(answer, why),
        };
        let Some(mut times) = world.get_mut::<Asked>(call.session)
        else {
            return;
        };
        // A call counts once, however many questions it holds.
        if times.questions >= asking.questions {
            let times = match asking.questions {
                1 => "once".to_string(),
                n => format!("{n} times"),
            };
            return deny(
                answer,
                format!(
                    "you have asked the chief {times} this turn; go \
                     on with what you know"
                ),
            );
        }
        times.questions += 1;
        let item = raise(
            world,
            Kind::Question,
            &call.who,
            asked.about(),
            Some(call.session),
            vec![Choice::Reply, Choice::Dismiss],
        );
        world
            .entity_mut(item)
            .insert((Held(vec![answer]), Asks(asked)));
        return;
    }

    // An act the limits reserve to the chief waits like anything else
    // only a chief may allow.
    let roads = serde_json::from_str::<Value>(&call.args)
        .ok()
        .and_then(|v| v["roads"].as_array().map(|r| !r.is_empty()))
        .unwrap_or(false);
    let verdict = if call.who == MAYOR
        && danger::reserved(&call.name, roads, &limits.mayor)
    {
        Verdict::Ask(Danger::Reserved)
    } else {
        let standing =
            standing(world, &limits, &call.who, call.session);
        let words = &world.resource::<Rules>().words;
        danger::judge(&call.reach, standing, words)
    };
    let danger = match verdict {
        Verdict::Run(leave) => {
            let _ = answer.send(Ruling::Go(leave));
            return;
        }
        Verdict::Ask(danger) => danger,
    };
    let about = match &call.reach {
        Reach::Shell { command, .. } => command.clone(),
        Reach::Edit => edit_as_read(&call),
        _ => format!("{} {}", call.name, call.args),
    };
    let refused = Refused::of(danger, &about);
    let Some(asked) = world.get::<Asked>(call.session) else {
        return;
    };
    if let Some((_, _, reason)) = asked
        .refused
        .iter()
        .find(|(who, what, _)| *who == call.who && *what == refused)
    {
        return deny(
            answer,
            format!(
                "the chief already refused this in this turn \
                 ({reason}). Do not ask again: go on without it, or \
                 report that you could not."
            ),
        );
    }
    if asked.strikes >= asking.refusals {
        return deny(
            answer,
            format!(
                "the chief has refused {} requests this turn and \
                 will not be asked again in it. Stop and report \
                 what you could not do.",
                asked.strikes
            ),
        );
    }
    // The same call from the same agent waits as one thing.
    let same = world
        .query::<(Entity, &Waiting)>()
        .iter(world)
        .find(|(_, w)| {
            w.kind == Kind::Approval(danger)
                && w.by == call.who
                && w.about == about
        })
        .map(|(e, _)| e);
    let item = match same {
        Some(item) => {
            if let Some(mut waiting) = world.get_mut::<Waiting>(item)
            {
                waiting.asked += 1;
            }
            item
        }
        None => {
            let mut answers = vec![Choice::Allow];
            if grants::lifting(danger).is_some()
                || danger.for_session()
            {
                answers.push(Choice::Always);
            }
            answers.push(Choice::Deny);
            raise(
                world,
                Kind::Approval(danger),
                &call.who,
                about,
                Some(call.session),
                answers,
            )
        }
    };
    world
        .entity_mut(item)
        .entry::<Held>()
        .or_default()
        .into_mut()
        .0
        .push(answer);
}
