//! A chief's oversight: the questions and objections an agent puts
//! to a chief, the waits an agent closes, the spending the realm
//! holds, and the grants a chief takes back.

use std::fmt;

use hamlet_actor::Action;
use serde::{Deserialize, Serialize};

use super::{
    AgentRefusal, ChiefRequest, Done, SocietyTool, ToolCall,
};
use crate::Message;
use crate::address::{Agent, Realm};
use crate::workspace::{AgentNumber, ConversationId, RealmTask};

/// Closes the agent's open waits in a conversation, or all of them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lapse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation: Option<ConversationId>,
    pub why: LapseReason,
}

impl Action for Lapse {
    type Output = ();
    const NAME: &'static str = "lapse";
}

impl Message for Lapse {
    type To = Realm;
    type From = Agent;
}

/// The reason the agent's waits lapse.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum LapseReason {
    /// A chief stopped the agent's turn.
    Stopped,
    /// The conversation was taken back past the waits.
    Rewound,
    /// The agent ended.
    Ended,
    /// A chief went back to an earlier session, which ends the trust
    /// given for this one.
    Resumed,
}

/// The reason the realm holds an agent's spending.
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
pub enum Hold {
    #[error(
        "task {} is held on an objection until a chief says go or \
         withdraw",
        task.0
    )]
    Objection { task: RealmTask },
    #[error(
        "The chief withdrew the instruction, so this call did not \
         run."
    )]
    Withdrawn { task: RealmTask },
}

/// Takes back an agent's grant.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Revoke {
    pub agent: AgentNumber,
    pub permission: Permission,
}

impl ChiefRequest for Revoke {
    const NAME: &'static str = "chief_revoke";
    type Answer = Revoked;
}

answer!(
    /// The answer of a [`Revoke`]: a notice to the chief, in its
    /// `Display`, unless it is refused.
    Revoked<RevokeRefusal> {
        Revoked { name: String, permission: Permission },
        NoGrant { name: String, permission: Permission },
        /// No grant, and the settings give the network to every agent.
        AllowedToAll { name: String, permission: Permission },
    }
);

impl fmt::Display for Revoked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Revoked { name, permission } => {
                write!(f, "{name} may no longer use {permission}.")
            }
            Self::NoGrant { name, permission } => {
                write!(f, "{name} holds no grant of {permission}.")
            }
            Self::AllowedToAll { name, permission } => write!(
                f,
                "{name} holds no grant of {permission}; the \
                 settings allow every agent the network."
            ),
            Self::Refused { why } => why.fmt(f),
        }
    }
}

/// A grant a chief gives an agent beyond its hamlet's mode.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    Network,
}

impl fmt::Display for Permission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Network => "network",
        })
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
pub enum RevokeRefusal {
    #[error("no agent has the address `{address}`")]
    NoSuchAgent { address: String },
}

/// The most questions one [`AskChief`] asks.
pub const MOST_QUESTIONS: u32 = 10;

/// The most options one [`Question`] offers.
pub const MOST_OPTIONS: u32 = 6;

/// Asks the chief questions the agent cannot go on without. The
/// answer comes later as an input to `conversation`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AskChief {
    pub task: RealmTask,
    pub conversation: ConversationId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub questions: Vec<Question>,
}

impl ToolCall for AskChief {
    const TOOL: SocietyTool = SocietyTool::AskChief;
    type Answer = Done<AgentRefusal<AskChiefRefusal>>;
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Question {
    pub question: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<Offer>,
    /// Whether the chief may answer in words of its own.
    pub text_input: bool,
}

/// An option a question offers the chief.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Offer {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
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
pub enum AskChiefRefusal {
    #[error("no chief oversees this turn; go on with what you know")]
    NoChief,
    #[error(
        "you have asked the chief {} this turn; go on with what you \
         know",
        times(*most)
    )]
    TooMany { most: u32 },
    #[error("say what in `questions`")]
    NoQuestions,
    #[error("ask at most {MOST_QUESTIONS} questions at once")]
    TooManyQuestions,
    /// A question is counted from one.
    #[error("question {question} has no `question`")]
    Blank { question: u32 },
    #[error(
        "question {question} offers more than {MOST_OPTIONS} options"
    )]
    Crowded { question: u32 },
    #[error("an option of question {question} has no `title`")]
    Untitled { question: u32 },
    #[error("question {question} offers `{title}` twice")]
    Twice { question: u32, title: String },
}

fn times(most: u32) -> String {
    match most {
        1 => "once".into(),
        most => format!("{most} times"),
    }
}

/// Objects to the chief's instruction the agent works on, which holds
/// its task until a chief says go or withdraw.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Object {
    pub task: RealmTask,
    pub conversation: ConversationId,
    pub reason: String,
}

impl ToolCall for Object {
    const TOOL: SocietyTool = SocietyTool::Object;
    type Answer = Done<AgentRefusal<ObjectRefusal>>;
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
pub enum ObjectRefusal {
    #[error(
        "only a chief's instruction can be objected to, in the turn \
         that works on it"
    )]
    NotAChiefsLine,
    #[error("say why you object")]
    NoReason,
    #[error("you have objected to this instruction already")]
    AlreadyHeld,
}
