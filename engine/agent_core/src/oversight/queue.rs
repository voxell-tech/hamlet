//! The queue of what waits for a chief: each thing, the agent that
//! raised it and its one-line form.

use std::time::{Duration, Instant};

use bevy_ecs::prelude::*;
use hamlet_model::log;
use hamlet_rules::chief::asking::Questions;
use hamlet_rules::chief::rulings::{Choice, Party};
use hamlet_rules::words::clip;

use crate::agents::{self, Citizen};
use crate::oversight::danger::Danger;
use crate::oversight::ledger::Ledger;

/// The kind of thing that waits for a chief.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Approval(Danger),
    Objection(Party),
    Question,
    Fault,
    /// Something that waited when the app last quit and was never
    /// answered.
    Lapsed,
}

impl Kind {
    /// Its name in the record.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Approval(_) => "approval",
            Kind::Objection(_) => "objection",
            Kind::Question => "question",
            Kind::Fault => "fault",
            Kind::Lapsed => "lapsed",
        }
    }
}

/// Something that waits for a chief.
#[derive(Component, Clone, Debug)]
pub struct Waiting {
    /// Its name in the record.
    pub id: String,
    pub kind: Kind,
    /// The name of the agent that raised it.
    pub by: String,
    /// The exact text the chief has to read.
    pub about: String,
    pub since: Instant,
    /// Its place in the queue. Lower is older.
    pub order: u64,
    /// Times it was asked while it waited.
    pub asked: u32,
    pub answers: Vec<Choice>,
    /// The session whose turn it holds.
    pub session: Option<Entity>,
}

/// One line for a strip or a tag, with at most `max` characters of
/// `about`.
pub(crate) fn line(
    kind: Kind,
    by: &str,
    about: &str,
    max: usize,
) -> String {
    match kind {
        // An edit opens with the file it is of.
        Kind::Approval(Danger::Edit) => format!(
            "{by} {} `{}`",
            Danger::Edit.says(),
            clip(about.lines().next().unwrap_or(about), max)
        ),
        Kind::Approval(danger) => {
            format!("{by} {} `{}`", danger.says(), clip(about, max))
        }
        // The objection follows the instruction it is about.
        Kind::Objection(_) => clip(
            about.lines().nth(1).unwrap_or(about),
            max + by.len(),
        ),
        Kind::Question => format!("{by} asks: {}", clip(about, max)),
        Kind::Fault => format!("{by} failed: {}", clip(about, max)),
        Kind::Lapsed => {
            format!("{by}, unanswered: {}", clip(about, max))
        }
    }
}

impl Waiting {
    /// One line for it, with at most `max` characters of `about`.
    pub fn line(&self, max: usize) -> String {
        line(self.kind, &self.by, &self.about, max)
    }

    /// The time it has waited.
    pub fn waited(&self) -> Duration {
        self.since.elapsed()
    }
}

/// The questions a [`Kind::Question`] asks, as the agent put them.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct Asks(pub Questions);

/// The agent that raised something that waits. Absent when no
/// living agent goes by the name it was raised under.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
#[relationship(relationship_target = Needs)]
pub struct RaisedBy(pub Entity);

/// The things an agent has waiting for a chief.
#[derive(Component, Default, Debug)]
#[relationship_target(relationship = RaisedBy)]
pub struct Needs(Vec<Entity>);

impl Needs {
    /// The things that wait.
    pub fn iter(&self) -> impl Iterator<Item = Entity> + '_ {
        self.0.iter().copied()
    }
}

/// The count of things raised so far.
#[derive(Resource, Default)]
pub(super) struct Raised(u64);

/// Something that waits, as a panel shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Need {
    pub entity: Entity,
    pub kind: Kind,
    pub by: String,
    pub about: String,
    /// The time it has waited, in words.
    pub waited: String,
    pub asked: u32,
    pub answers: Vec<Choice>,
    /// The questions it asks, when it is a question.
    pub questions: Option<Questions>,
}

impl Need {
    /// `waiting` as a panel shows it now, with the questions it
    /// `asks`.
    pub fn of(
        entity: Entity,
        waiting: &Waiting,
        asks: Option<&Asks>,
    ) -> Self {
        Need {
            entity,
            kind: waiting.kind,
            by: waiting.by.clone(),
            about: waiting.about.clone(),
            waited: waited(waiting.waited()),
            asked: waiting.asked,
            answers: waiting.answers.clone(),
            questions: asks.map(|asks| asks.0.clone()),
        }
    }

    /// One line for it, with at most `max` characters of `about`.
    pub fn line(&self, max: usize) -> String {
        line(self.kind, &self.by, &self.about, max)
    }
}

/// Puts something in the queue and notes it in the record.
pub(super) fn raise(
    world: &mut World,
    kind: Kind,
    by: &str,
    about: String,
    session: Option<Entity>,
    answers: Vec<Choice>,
) -> Entity {
    let order = {
        let mut raised = world.resource_mut::<Raised>();
        raised.0 += 1;
        raised.0
    };
    let id = format!("{}-{order}", log::now_millis());
    let agent = agents::named(world, by);
    let number = agent
        .and_then(|agent| world.get::<Citizen>(agent))
        .map(|citizen| citizen.0);
    // The record is for the chief to read later; failing to write
    // it must not let anything through.
    let _ = world.resource::<Ledger>().raised(
        &id,
        kind.name(),
        by,
        number,
        &about,
    );
    let mut item = world.spawn(Waiting {
        id,
        kind,
        by: by.to_string(),
        about,
        since: Instant::now(),
        order,
        asked: 1,
        answers,
        session,
    });
    if let Some(agent) = agent {
        item.insert(RaisedBy(agent));
    }
    item.id()
}

/// The number of the agent that raised `item`.
pub(super) fn raiser(world: &World, item: Entity) -> Option<u64> {
    let agent = world.get::<RaisedBy>(item)?.0;
    world.get::<Citizen>(agent).map(|citizen| citizen.0)
}

/// Takes `entity` out of the queue and notes how it ended.
pub(super) fn close(
    world: &mut World,
    entity: Entity,
    how: &str,
    text: &str,
    chief: Option<&str>,
) {
    if let Some(waiting) = world.get::<Waiting>(entity) {
        let _ = world.resource::<Ledger>().settled(
            &waiting.id,
            how,
            text,
            chief,
        );
    }
    if let Ok(entity) = world.get_entity_mut(entity) {
        entity.despawn();
    }
}

/// The time `d` in words.
fn waited(d: Duration) -> String {
    match d.as_secs() {
        0..=59 => "under a minute".into(),
        s @ 60..=3599 => format!("{} min", s / 60),
        s => format!("{} h {:02} min", s / 3600, s % 3600 / 60),
    }
}
