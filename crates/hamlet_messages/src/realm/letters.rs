//! Letters between agents: `tell`, `ask` and a reply to an ask.

use std::path::PathBuf;

use hamlet_actor::Action;
use serde::{Deserialize, Serialize};

use super::{
    AgentName, AgentRefusal, Done, LetterId, Named, SocietyTool,
    ToolCall,
};
use crate::Message;
use crate::address::{Agent, Realm};
use crate::workspace::ConversationId;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tell {
    pub to: Named,
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<Enclosed>,
}

/// A file or a folder a letter sends, in the sender's filesystem
/// or its hamlet's folder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Enclosed {
    pub path: PathBuf,
}

impl ToolCall for Tell {
    const TOOL: SocietyTool = SocietyTool::Tell;
    type Answer = Sent<AgentRefusal<LetterRefusal>>;
}

answer!(
    /// The answer of a [`Tell`].
    Sent<W> {
        Sent { to: AgentName },
    }
);

/// A question; its answer arrives later as an input to
/// `conversation`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ask {
    pub to: Named,
    pub text: String,
    pub conversation: ConversationId,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<Enclosed>,
}

impl ToolCall for Ask {
    const TOOL: SocietyTool = SocietyTool::Ask;
    type Answer = Waiting<AgentRefusal<LetterRefusal>>;
}

answer!(
    /// The answer of an [`Ask`]: `question` is the letter its answer
    /// names.
    Waiting<W> {
        Waiting { on: AgentName, question: LetterId },
    }
);

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
pub enum LetterRefusal {
    #[error("That is you.")]
    Yourself,
    #[error("Say what to send.")]
    Empty,
    #[error("no agent has the address `{address}`")]
    NoSuchAgent { address: String },
    #[error(
        "You have no road to {to}. Ask your lead for one with \
         request_road."
    )]
    NoRoad { to: AgentName },
    #[error("no road yet to `{to}`: it is still being hired")]
    StillHiring { to: AgentName },
    #[error(
        "The chief withdrew the instruction, so this call did not \
         run."
    )]
    Withdrawn,
    /// The files it names cannot be sent, as the workspace measured
    /// them under the sender's reach.
    #[error("{words}")]
    Files { words: String },
}

/// The answer to a question, when the asked agent's turn on it ends.
/// The question is its key, and a repeat keeps the first.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reply {
    pub question: LetterId,
    pub text: String,
}

impl Action for Reply {
    type Output = Done<ReplyRefusal>;
    const NAME: &'static str = "reply";
}

impl Message for Reply {
    type To = Realm;
    type From = Agent;
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
pub enum ReplyRefusal {
    #[error("there is no such question")]
    NoSuchQuestion,
    #[error("the question was asked of another agent")]
    NotAsked,
}
