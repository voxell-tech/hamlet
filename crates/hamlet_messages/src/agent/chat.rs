//! The calls a client makes to an agent about its chat, and their
//! answers. A time in them is in whole seconds.

use hamlet_actor::Action;
use hamlet_protocol::client as protocol;
use serde::{Deserialize, Serialize};

use crate::address::Agent;
use crate::realm::Done;
use crate::sender::Client;
use crate::{Message, worded};

macro_rules! from_client {
    ($ty:ty, $name:literal, $out:ty) => {
        impl Action for $ty {
            type Output = $out;
            const NAME: &'static str = $name;
        }
        impl Message for $ty {
            type To = Agent;
            type From = Client;
        }
    };
}

/// Stops the agent's running turns and their calls out.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Stop(pub protocol::Stop);
from_client!(Stop, "stop", Stopped);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stopped {
    pub turns: u32,
}

/// Begins a new session in the chief's chat.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NewSession(pub protocol::New);
from_client!(NewSession, "new_session", Began);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Began {
    pub number: u32,
}

/// Goes back to an earlier session by its number.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Resume(pub protocol::Resume);
from_client!(Resume, "resume", ResumeAnswer);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResumeAnswer {
    Back {
        number: u32,
    },
    #[serde(serialize_with = "worded")]
    Refused {
        why: ResumeRefusal,
    },
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
pub enum ResumeRefusal {
    #[error("there is no session {number}")]
    NoSuchSession { number: u32 },
}

/// Carries on the current session's last turn, which failed or was
/// stopped (C26).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Retry(pub protocol::Retry);
from_client!(Retry, "retry", Done<RetryRefusal>);

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
pub enum RetryRefusal {
    #[error("a turn is running; stop it first")]
    Running,
    #[error(
        "the last turn was answered; there is nothing to carry on"
    )]
    Answered,
    #[error("this session has no turn to carry on")]
    NoTurn,
}

/// Takes a session back to before a chief's line.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Rewind(pub protocol::Rewind);
from_client!(Rewind, "rewind", RewindAnswer);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RewindAnswer {
    /// The line taken back, to return to the chief's input (C33).
    Rewound {
        text: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        images: Vec<protocol::AttachedImage>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        files: Vec<protocol::AttachedFile>,
    },
    #[serde(serialize_with = "worded")]
    Refused { why: RewindRefusal },
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
pub enum RewindRefusal {
    #[error("only a chief's own line can be taken back")]
    NotAChiefLine,
    #[error("no line {item} is in this session")]
    NoSuchItem { item: String },
    #[error("only the current session of a chief's chat goes back")]
    NotCurrent,
    #[error("work is running or waiting; stop it first")]
    Working,
}

/// Reads a page of a chat: a session of the chief's, or a peer's
/// letters or questions.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ReadChat(pub protocol::Open);
from_client!(ReadChat, "read_chat", ChatPage);

/// A page of a chat.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatPage {
    /// The agent's wake the numbers count in; a new wake starts them
    /// again.
    pub epoch: u32,
    /// The number of the agent's last change this page holds: a
    /// [`ChatEvent`] numbered at most this is already in it.
    pub number: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<u32>,
    pub items: Vec<ChatItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub earlier: Option<String>,
    pub working: bool,
    pub pending: u32,
    pub usage: protocol::UsageView,
}

/// One item of a chat, as the client protocol's `Item` but with its
/// time in whole seconds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatItem {
    pub id: String,
    pub at: u32,
    pub kind: protocol::ItemKind,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<protocol::AttachedImage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<protocol::AttachedFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rewindable: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<protocol::ItemTool>,
}

/// The event `chat` the agent publishes when a chat changes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatEvent {
    pub chat: protocol::ChatRef,
    pub epoch: u32,
    /// One more than the agent's last change before it, in this
    /// epoch.
    pub number: u32,
    pub ops: Vec<ChatOp>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatOp {
    Add {
        item: Box<ChatItem>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        replaces: Option<String>,
    },
    Append {
        item: String,
        /// The place in the item's text `text` is written at, so a
        /// repeat writes the same.
        at: u32,
        text: String,
    },
    Status {
        working: bool,
        pending: u32,
    },
    Usage {
        usage: protocol::UsageView,
    },
    /// The chat must be read again.
    Reset,
}

/// Lists the sessions of the chief's chat, newest first.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadHistory;
from_client!(ReadHistory, "read_history", History);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct History {
    pub sessions: Vec<Session>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub number: u32,
    pub began: u32,
    pub modified: u32,
    pub current: bool,
    pub messages: u32,
    pub tokens: u32,
    pub title: String,
}

/// Reads the agent's whole system prompt as it would be sent now
/// (C14).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadPrompt;
from_client!(ReadPrompt, "read_prompt", Prompt);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prompt {
    pub text: String,
}

/// Asks the agent to reflect now (`/reflect`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reflect;
from_client!(Reflect, "reflect", Reflecting);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reflecting {
    Asked,
    Already,
}

/// A step of a sign-in to a provider, which a client makes on the
/// Hand.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Login(pub protocol::Login);
from_client!(Login, "login", LoginAnswer);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LoginAnswer {
    Step {
        login: String,
        step: protocol::LoginStep,
    },
    Done {
        login: String,
    },
    Failed {
        login: String,
        words: String,
    },
}
