//! The things waiting for a chief and their presentation.

use std::time::Duration;

use bevy_ecs::prelude::*;
use hamlet_protocol::client as protocol;
pub use protocol::Choice;

/// The questions of a received ask_chief call.
#[derive(Component, Clone, Debug)]
pub struct Asks(pub protocol::Questions);

/// The server's refusal of the chief's latest answer to a need.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct Refused(pub String);

/// A received need and the answers offered by the server.
#[derive(Component, Clone, Debug)]
#[component(
    immutable,
    on_insert = crate::index::file_wait,
    on_discard = crate::index::unfile_wait
)]
pub struct Waiting {
    pub id: String,
    pub by: String,
    pub about: String,
    /// The line of `about` a one-line view quotes.
    pub subject: String,
    pub change: Option<protocol::Change>,
    pub answers: Vec<Choice>,
    /// Milliseconds since 1970.
    pub since: u64,
    pub asked: u64,
    pub chat: Option<protocol::ChatRef>,
    pub detail: protocol::WaitDetail,
}

impl Waiting {
    /// One line of at most about `max` characters of the subject.
    pub fn line(&self, max: usize) -> String {
        let flat = self
            .subject
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let objection =
            matches!(self.detail, protocol::WaitDetail::Objection {});
        // An objection's line has no name before it, so its subject
        // takes the name's room too.
        let max = if objection { max + self.by.len() } else { max };
        let clipped = match flat.char_indices().nth(max) {
            Some((cut, _)) => format!("{}...", &flat[..cut]),
            None => flat,
        };
        match &self.detail {
            protocol::WaitDetail::Approval { says, .. } => {
                format!("{} {says} `{clipped}`", self.by)
            }
            protocol::WaitDetail::Objection {} => clipped,
            protocol::WaitDetail::Question { .. } => {
                format!("{} asks: {clipped}", self.by)
            }
            protocol::WaitDetail::Fault {} => {
                format!("{} failed: {clipped}", self.by)
            }
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

pub(crate) fn find(world: &World, id: &str) -> Option<Entity> {
    world.get_resource::<crate::index::Index>()?.wait(id)
}

pub(crate) fn upsert(
    world: &mut World,
    wait: protocol::Wait,
) -> Entity {
    let entity = find(world, &wait.id)
        .unwrap_or_else(|| world.spawn_empty().id());
    let agent = wait
        .agent
        .as_ref()
        .and_then(|agent| crate::realm::agent(world, agent));
    let mut entity = world.entity_mut(entity);
    if let protocol::WaitDetail::Question { questions } = &wait.detail
    {
        entity.insert(Asks(questions.clone()));
    }
    entity.insert(Waiting {
        id: wait.id,
        by: wait.by,
        about: wait.about,
        subject: wait.subject,
        change: wait.change,
        answers: wait.answers,
        since: wait.since,
        asked: wait.asked,
        chat: wait.chat,
        detail: wait.detail,
    });
    if let Some(agent) = agent {
        entity.insert(RaisedBy(agent));
    } else {
        entity.remove::<RaisedBy>();
    }
    entity.id()
}
