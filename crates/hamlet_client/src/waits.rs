//! The things waiting for a chief and their presentation.

use std::time::Duration;

use bevy_ecs::prelude::*;
use hamlet_protocol::v0;
pub use v0::Choice;

/// The reason a tool call needs approval.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Danger {
    Network,
    Unconfined,
    Destructive,
    Reserved,
    Edit,
    Command,
}

impl Danger {
    /// Eligibility for approval lasting through this conversation.
    pub fn for_session(self) -> bool {
        matches!(self, Self::Edit | Self::Command)
    }
}

impl From<v0::WaitDanger> for Danger {
    fn from(danger: v0::WaitDanger) -> Self {
        match danger {
            v0::WaitDanger::Network => Self::Network,
            v0::WaitDanger::Unconfined => Self::Unconfined,
            v0::WaitDanger::Destructive => Self::Destructive,
            v0::WaitDanger::Reserved => Self::Reserved,
            v0::WaitDanger::Edit => Self::Edit,
            v0::WaitDanger::Command => Self::Command,
        }
    }
}

/// The kind of need raised for a chief.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Approval(Danger),
    Objection,
    Question,
    Fault,
}

/// A received need and the answers offered by the server.
#[derive(Component, Clone, Debug)]
pub struct Waiting {
    pub id: String,
    pub kind: Kind,
    pub by: String,
    pub says: String,
    pub about: String,
    pub change: Option<v0::Change>,
    pub answers: Vec<Choice>,
    /// Milliseconds since 1970.
    pub since: u64,
    pub asked: u32,
    pub chat: Option<v0::ChatRef>,
}

impl Waiting {
    /// One clipped subject line, with the speaker's width for
    /// objections.
    pub fn line(&self, max: usize) -> String {
        let about = match self.kind {
            Kind::Approval(Danger::Edit) => {
                self.about.lines().next().unwrap_or_default()
            }
            Kind::Objection => {
                self.about.lines().nth(1).unwrap_or(&self.about)
            }
            _ => &self.about,
        };
        let flat =
            about.split_whitespace().collect::<Vec<_>>().join(" ");
        let max = if self.kind == Kind::Objection {
            max + self.by.len()
        } else {
            max
        };
        let clipped = match flat.char_indices().nth(max) {
            Some((cut, _)) => format!("{}...", &flat[..cut]),
            None => flat,
        };
        match self.kind {
            Kind::Approval(_) => {
                format!("{} {} `{clipped}`", self.by, self.says)
            }
            Kind::Objection => clipped,
            Kind::Question => format!("{} asks: {clipped}", self.by),
            Kind::Fault => format!("{} failed: {clipped}", self.by),
        }
    }

    /// The elapsed time since this need was raised.
    pub fn waited(&self, now_ms: u64) -> Duration {
        Duration::from_millis(now_ms.saturating_sub(self.since))
    }
}

/// The agent that raised a need.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[relationship(relationship_target = Needs)]
pub struct RaisedBy(pub Entity);

/// The needs raised by an agent.
#[derive(Component, Default, Debug)]
#[relationship_target(relationship = RaisedBy)]
pub struct Needs(Vec<Entity>);

impl Needs {
    /// The need entities raised by this agent.
    pub fn iter(&self) -> impl Iterator<Item = Entity> + '_ {
        self.0.iter().copied()
    }
}

/// Presentation for the protocol's answers.
pub trait ChoiceExt {
    /// The first offered answer matching agreement or refusal.
    fn quick(answers: &[Choice], agree: bool) -> Option<Choice>;
}

impl ChoiceExt for Choice {
    fn quick(answers: &[Choice], agree: bool) -> Option<Choice> {
        let plain = if agree {
            [Choice::Allow, Choice::Go]
        } else {
            [Choice::Deny, Choice::Withdraw]
        };
        answers
            .iter()
            .find(|answer| plain.contains(answer))
            .or(answers.first().filter(|answer| {
                agree && **answer == Choice::Dismiss
            }))
            .cloned()
    }
}

pub(crate) fn find(world: &mut World, id: &str) -> Option<Entity> {
    world
        .query::<(Entity, &Waiting)>()
        .iter(world)
        .find(|(_, wait)| wait.id == id)
        .map(|(entity, _)| entity)
}

pub(crate) fn upsert(world: &mut World, wait: v0::Wait) -> Entity {
    let kind = match &wait {
        v0::Wait::Approval { danger, .. } => {
            Kind::Approval(danger.clone().into())
        }
        v0::Wait::Fault { .. } => Kind::Fault,
        v0::Wait::Question { .. } => Kind::Question,
        v0::Wait::Objection { .. } => Kind::Objection,
    };
    let (v0::Wait::Approval {
        id,
        by,
        says,
        about,
        change,
        answers,
        since,
        asked,
        chat,
        agent,
        ..
    }
    | v0::Wait::Fault {
        id,
        by,
        says,
        about,
        change,
        answers,
        since,
        asked,
        chat,
        agent,
        ..
    }
    | v0::Wait::Question {
        id,
        by,
        says,
        about,
        change,
        answers,
        since,
        asked,
        chat,
        agent,
        ..
    }
    | v0::Wait::Objection {
        id,
        by,
        says,
        about,
        change,
        answers,
        since,
        asked,
        chat,
        agent,
        ..
    }) = wait;
    let entity =
        find(world, &id).unwrap_or_else(|| world.spawn_empty().id());
    let agent = agent
        .as_ref()
        .and_then(|agent| crate::realm::agent(world, agent));
    let mut entity = world.entity_mut(entity);
    entity.insert(Waiting {
        id,
        kind,
        by,
        says,
        about,
        change,
        answers,
        since: since as u64,
        asked: asked as u32,
        chat,
    });
    if let Some(agent) = agent {
        entity.insert(RaisedBy(agent));
    } else {
        entity.remove::<RaisedBy>();
    }
    entity.id()
}
