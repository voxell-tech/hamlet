//! The realm's calls whose argument is the client protocol's own
//! type, wrapped to be a call of the realm. Each carries its own
//! request id, which marks a repeat.

use hamlet_actor::Action;
use hamlet_protocol::client as protocol;
use serde::{Deserialize, Serialize};

use super::world::{Revision, Seen};
use super::{AgentName, Done, Left, Refusable};
use crate::address::Realm;
use crate::sender::Client;
use crate::workspace::RealmTask;
use crate::{Message, worded};

/// A chief's line to an agent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Say(pub protocol::Say);

impl Action for Say {
    type Output = SayAnswer;
    const NAME: &'static str = "say";
}

impl Message for Say {
    type To = Realm;
    type From = Client;
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SayAnswer {
    /// The realm keeps the line until its agent takes it, a stop
    /// withdraws it, or the agent leaves.
    Accepted { task: RealmTask },
    #[serde(serialize_with = "worded")]
    Refused { why: SayRefusal },
}

impl Refusable for SayAnswer {
    type Why = SayRefusal;
    fn refused(why: SayRefusal) -> Self {
        Self::Refused { why }
    }
}

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SayRefusal {
    #[error("no agent has the address `{address}`")]
    NoSuchAgent { address: String },
    #[error("`{agent}` has left: {left}")]
    Left { agent: AgentName, left: Left },
}

/// A chief's answer to a wait.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Answer(pub protocol::Answer);

impl Action for Answer {
    type Output = Done<AnswerRefusal>;
    const NAME: &'static str = "answer";
}

impl Message for Answer {
    type To = Realm;
    type From = Client;
}

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AnswerRefusal {
    #[error("nothing waits here with that id")]
    NothingWaits { wait: String },
    #[error("that is not an answer to this wait")]
    NotOffered,
    #[error("Say what to reply.")]
    BlankReply,
    #[error("every question needs an answer")]
    Unequal,
    #[error("question {question} has no such option")]
    NoOption { question: u32 },
    #[error("question {question} takes an option")]
    TakesOption { question: u32 },
    #[error("question {question} needs words")]
    NoWords { question: u32 },
}

/// Reads the world a client shows. It answers while the realm
/// changes, from the copy of the last commit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadWorld;

impl Action for ReadWorld {
    type Output = Seen;
    const NAME: &'static str = "read_world";
}

impl Message for ReadWorld {
    type To = Realm;
    type From = Client;
}

/// Reads the world as the client protocol shows it: every part of the
/// `State` the realm holds, in the words a chief reads. The parts the
/// realm does not hold are empty: the server's own fields, the
/// library, the providers, and what an agent is doing and saying.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadState;

impl Action for ReadState {
    type Output = protocol::State;
    const NAME: &'static str = "read_state";
}

impl Message for ReadState {
    type To = Realm;
    type From = Client;
}

/// The realm's event, after each commit and each change of whether an
/// agent is working.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
pub struct World {
    pub revision: Revision,
}

impl hamlet_actor::Event for World {
    const NAME: &'static str = "world";
}
