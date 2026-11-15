//! The calls an `agent` actor takes from the other actors, and their
//! answers.

pub mod chat;
pub mod copies;

use std::path::PathBuf;

use hamlet_actor::Action;
use hamlet_protocol::client::{AttachedFile, AttachedImage};
use serde::{Deserialize, Serialize};

use crate::Message;
use crate::address::{Agent, Realm, Workspace};
use crate::realm::{LetterId, ProfileId, ToolName};
use crate::workspace::files::ChangeEnd;
use crate::workspace::{
    AgentNumber, CallId, ConversationId, RealmTask, RunEnd,
};

/// Tells the agent how a call ended that the workspace answered
/// `Running` or `WaitsForChief` for. Called by the workspace once,
/// when it ends, unless the workspace's host stopped first. A call
/// that fails is not made again.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finished {
    pub call: CallId,
    pub outcome: Outcome,
}

impl Action for Finished {
    type Output = ();
    const NAME: &'static str = "finished";
}

impl Message for Finished {
    type To = Agent;
    type From = Workspace;
}

/// The end of a call, by its kind: the `end` its answer would have
/// held under `ended`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Outcome {
    Run { end: RunEnd },
    Change { end: ChangeEnd },
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum AgentKind {
    Hand,
    Hire,
    Worker,
    Errand,
}

/// The input `agent/<n>` is created with.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentInput {
    pub kind: AgentKind,
    /// The task that pays for a turn no chief's line started.
    pub task: RealmTask,
    pub place: Place,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brief: Option<Brief>,
}

/// The one turn a worker or an errand runs when it is created.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Brief {
    /// An errand's whole prompt (C11); absent for a worker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    /// The turn's input.
    pub text: String,
    /// The built-in tools it may use. Empty is its starter's for a
    /// worker, and none for an errand.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<BuiltIn>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// One of an agent's built-in tools, by name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuiltIn {
    pub name: String,
}

/// The id an input is taken under. A second [`Submit`] with the same
/// id is the first one.
#[derive(
    Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub struct InputId(pub String);

/// Gives the agent an input to work on.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Submit {
    pub id: InputId,
    /// The realm's task the turn is charged to; absent for a letter
    /// whose sender's turn had none, which the standing task pays.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<RealmTask>,
    pub input: Input,
}

impl Action for Submit {
    type Output = ();
    const NAME: &'static str = "submit";
}

impl Message for Submit {
    type To = Agent;
    type From = Realm;
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Input {
    /// A chief's line, with the attachments as the client sent them.
    Line {
        text: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        images: Vec<AttachedImage>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        files: Vec<AttachedFile>,
        /// The note of the lead who reviewed the line.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
    /// A chief's ruling, or news the realm tells the agent.
    Notice {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        conversation: Option<ConversationId>,
    },
    /// The answer of a worker the agent started.
    WorkerAnswer {
        worker: AgentNumber,
        text: String,
    },
    Letter {
        from: Author,
        text: String,
    },
    /// A letter the agent answers with [`crate::realm::Reply`] when
    /// its turn on it ends.
    Question {
        letter: LetterId,
        from: Author,
        text: String,
    },
    /// The answer to a question the agent asked, for the conversation
    /// it asked from.
    Answer {
        question: LetterId,
        from: Author,
        text: String,
        conversation: ConversationId,
    },
}

/// The writer of a letter, as the receiver is shown it (C30).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Author {
    pub agent: AgentNumber,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// The realm's version of one agent's place, from 1.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
)]
pub struct PlaceVersion(pub u64);

/// An agent's place in the tree.
/// The agent keeps it when `version` is newer than the one it holds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Place {
    pub version: PlaceVersion,
    /// Absent for the Hand and a mayor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead: Option<AgentNumber>,
    pub hires: Vec<AgentNumber>,
    /// The agents it has a road to, its lead and its hires left out.
    pub roads: Vec<AgentNumber>,
    pub tools_off: Vec<ToolName>,
    /// Its hamlet's project folder, where its commands run and a
    /// relative path starts; absent when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<PathBuf>,
}

impl Action for Place {
    type Output = ();
    const NAME: &'static str = "place";
}

impl Message for Place {
    type To = Agent;
    type From = Realm;
}

/// Fills the agent's filesystem from `branch` of `profile`. An agent
/// already filled answers the same.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fill {
    pub profile: ProfileId,
    pub branch: String,
}

impl Action for Fill {
    type Output = ();
    const NAME: &'static str = "fill";
}

impl Message for Fill {
    type To = Agent;
    type From = Realm;
}
