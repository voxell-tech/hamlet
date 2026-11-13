//! The calls the `workspace` actor takes, and its answers.

pub mod reach;

use std::path::PathBuf;

use hamlet_actor::Action;
use serde::{Deserialize, Serialize};

use self::reach::Reach;

/// The number in an agent actor's file name,
/// `actors/agent/<n>.sqlite`, and its key.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub struct AgentNumber(pub u64);

/// The id an agent gives a tool call. A repeat of the call carries
/// the same id.
#[derive(
    Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub struct CallId(pub String);

/// A conversation of an agent, which trust is given for (H11).
#[derive(
    Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub struct ConversationId(pub String);

/// The realm's task a call is made for, which counts a chief's
/// refusals. Not the id of the call's own durable run.
#[derive(
    Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub struct RealmTask(pub String);

/// The realm's version of one agent's reach, from 1.
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
pub struct ReachVersion(pub u64);

/// Sets an agent's reach when `version` is newer than the one in
/// force. Called by the realm.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetReach {
    pub agent: AgentNumber,
    pub version: ReachVersion,
    pub reach: Reach,
}

impl Action for SetReach {
    type Output = ReachAnswer;
    const NAME: &'static str = "set_reach";
}

/// The workspace's answer to a [`SetReach`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReachAnswer {
    /// The agent's version in force after the call.
    InForce(ReachVersion),
    Refused(ReachRefusal),
}

/// A [`SetReach`] the workspace does not take.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
pub enum ReachRefusal {
    #[error("only the realm sets a reach")]
    NotTheRealm,
}

/// A call of an agent's tool from a caller it is not for (W2).
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
pub enum CallerRefusal {
    #[error("only an agent calls for its tools")]
    NotAnAgent,
    #[error("agent {} called for agent {}", caller.0, named.0)]
    NotTheCaller {
        named: AgentNumber,
        caller: AgentNumber,
    },
}

/// The shell a command's script is written for.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
pub enum Shell {
    Bash,
}

/// Runs a script for an agent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    pub agent: AgentNumber,
    pub call: CallId,
    pub conversation: ConversationId,
    /// `None` when no chief's instruction started the work.
    pub realm_task: Option<RealmTask>,
    pub shell: Shell,
    pub script: String,
    /// The model said the command needs the network (C19).
    pub network: bool,
    /// Shortens the 120 s limit (C16), in milliseconds.
    pub timeout_ms: Option<u64>,
}

/// Creates or replaces a whole file for an agent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WriteFile {
    pub agent: AgentNumber,
    pub call: CallId,
    pub conversation: ConversationId,
    /// `None` when no chief's instruction started the work.
    pub realm_task: Option<RealmTask>,
    pub path: PathBuf,
    pub content: String,
}

/// Makes exact text replacements in one file for an agent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditFile {
    pub agent: AgentNumber,
    pub call: CallId,
    pub conversation: ConversationId,
    /// `None` when no chief's instruction started the work.
    pub realm_task: Option<RealmTask>,
    pub path: PathBuf,
    pub edits: Vec<Replace>,
}

/// One replacement of an [`EditFile`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Replace {
    pub old: String,
    pub new: String,
}
