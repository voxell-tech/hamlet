//! An agent's reach on a machine, as the realm sets it (C18 to C21,
//! H11).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::{AgentNumber, ConversationId};

/// The places one agent's tool calls may touch on this machine, and
/// when they wait for a chief.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Reach {
    Agent(Footing),
    /// `starter` is `None` for a chief's worker (H42).
    Worker {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        starter: Option<AgentNumber>,
        footing: Footing,
    },
    /// A dream's errand, which works in `main`, the checkout of its
    /// profile's main inside the realm folder, also writes `hire`,
    /// the checkout of a hire's branch, and never waits for a chief.
    Dream {
        main: PathBuf,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hire: Option<PathBuf>,
        /// Its hire's network, or the Hand's for a profile's dream.
        network: bool,
        secrets: SecretsVersion,
    },
    /// The agent ended. Its row stays, so an older reach that arrives
    /// late cannot bring it back.
    Ended,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Footing {
    /// The hamlet's folder (C20). `None` for the Hand, a hire of a
    /// hamlet with no folder, and their workers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Manual,
    Auto,
}

/// The acts a chief allowed for the rest of one conversation.
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

/// One thing a call waits for a chief to allow (H11).
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Need {
    Network,
    Command,
    Edit,
}

impl std::fmt::Display for Need {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        f.write_str(match self {
            Self::Network => "the network",
            Self::Command => "a command",
            Self::Edit => "an edit",
        })
    }
}

pub(crate) fn needs_words(needs: &[Need]) -> String {
    let words =
        needs.iter().map(ToString::to_string).collect::<Vec<_>>();
    match words.split_last() {
        None => String::new(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => {
            format!("{} and {last}", rest.join(", "))
        }
    }
}

impl Reach {
    /// The footing of an agent's or a worker's reach.
    pub fn footing(&self) -> Option<&Footing> {
        match self {
            Self::Agent(footing) | Self::Worker { footing, .. } => {
                Some(footing)
            }
            Self::Dream { .. } | Self::Ended => None,
        }
    }

    /// Whether its programs reach the network without asking.
    pub fn network(&self) -> bool {
        match self {
            Self::Dream { network, .. } => *network,
            _ => {
                self.footing().is_some_and(|footing| footing.network)
            }
        }
    }

    /// The version of the realm's secrets in force, for a reach that
    /// has not ended.
    pub fn secrets(&self) -> Option<SecretsVersion> {
        match self {
            Self::Dream { secrets, .. } => Some(*secrets),
            _ => self.footing().map(|footing| footing.secrets),
        }
    }
}
