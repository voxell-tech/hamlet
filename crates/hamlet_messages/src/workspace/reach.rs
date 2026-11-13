//! An agent's reach on a machine, as the realm sets it (C18 to C21,
//! H11), and why a call does not pass it.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::{AgentNumber, ConversationId};

/// What one agent's tool calls may touch on this machine, and when
/// they wait for a chief.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Reach {
    Agent(Footing),
    /// `starter` is `None` for a chief's worker (H42).
    Worker {
        starter: Option<AgentNumber>,
        footing: Footing,
    },
    /// The agent ended. Its row stays, so an older reach that arrives
    /// late cannot bring it back.
    Ended,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Footing {
    /// The hamlet's folder (C20). `None` for the Hand, a hire of a
    /// hamlet with no folder, and their workers.
    pub project: Option<Project>,
    /// The network for every agent, or a lasting grant (C19).
    pub network: bool,
    /// Trust for the rest of a conversation (H11).
    pub trust: Vec<Trust>,
    /// The version of the realm's secrets in force for this agent.
    /// No call passes while the workspace holds an older list (C32).
    pub secrets: SecretsVersion,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub folder: PathBuf,
    pub mode: Mode,
}

/// A hamlet's mode on its folder (C21).
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
pub enum Mode {
    Manual,
    Auto,
}

/// What a chief allowed for the rest of one conversation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trust {
    pub conversation: ConversationId,
    pub edits: bool,
    pub commands: bool,
}

/// The realm's version of its list of secrets.
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
pub struct SecretsVersion(pub u64);

/// What a call waits for a chief to allow (H11).
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
pub enum Need {
    Network,
    Command,
    Edit,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
pub enum Access {
    Read,
    Change,
}

/// A call the gate does not let run.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
pub enum Refusal {
    #[error("{} is out of this agent's reach to {}", path.display(), match access {
        Access::Read => "read",
        Access::Change => "change",
    })]
    OutsideReach { path: PathBuf, access: Access },
    /// Only the agent itself changes its filesystem, and only a
    /// worker reads its starter's through the workspace.
    #[error("{} is in an agent's filesystem", path.display())]
    AgentFilesystem { path: PathBuf },
    #[error("this agent has no reach on this machine")]
    NoReach,
}

impl Reach {
    /// The footing of a reach that has not ended.
    pub fn footing(&self) -> Option<&Footing> {
        match self {
            Self::Agent(footing) | Self::Worker { footing, .. } => {
                Some(footing)
            }
            Self::Ended => None,
        }
    }
}
