//! The `workspace` actor, one per machine: it runs the agents'
//! commands and file changes under the sandbox, within each agent's
//! reach.

pub mod reach;

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use self::reach::Op;

/// The workspace's key.
pub const MACHINE: &str = "local";

/// The number in an agent actor's file name,
/// `actors/agent/<n>.sqlite`.
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
    /// Shortens the 120 s limit (C16).
    pub timeout: Option<Duration>,
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

impl Run {
    pub fn op(&self) -> Op<'_> {
        Op::Command {
            conversation: &self.conversation,
            script: &self.script,
            network: self.network,
        }
    }
}

impl WriteFile {
    pub fn op(&self) -> Op<'_> {
        Op::Change {
            conversation: &self.conversation,
            path: &self.path,
        }
    }
}

impl EditFile {
    pub fn op(&self) -> Op<'_> {
        Op::Change {
            conversation: &self.conversation,
            path: &self.path,
        }
    }
}
