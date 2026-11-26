//! The calls about an agent's chat, a client's but for the realm's
//! stop, and their answers. A time in them is in whole seconds, apart
//! from an item's, which is in milliseconds.

use hamlet_actor::Action;
use hamlet_protocol::client as protocol;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::address::{Agent, Realm};
use crate::agent::copies::Price;
use crate::agent::{Input, InputId};
use crate::bytes::Sha256;
use crate::realm::Done;
use crate::sender::Client;
use crate::workspace::AgentNumber;
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

/// Stops the agent's running turns and their calls out, after the
/// lines it was given, and writes the lines the realm withdrew into
/// the chat for display. A repeat of its number is answered as the
/// first.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stop {
    pub number: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub withdrawn: Vec<Withdrawn>,
}

impl Action for Stop {
    type Output = Stopped;
    const NAME: &'static str = "stop";
}

impl Message for Stop {
    type To = Agent;
    type From = Realm;
}

/// A chief's line the realm withdrew before its agent took it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Withdrawn {
    pub id: InputId,
    pub line: Input,
}

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
    #[error("there is no past session {number} here")]
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
    #[error("a turn is running here; stop it, then retry")]
    Running,
    #[error(
        "the last message was answered, so there is nothing to \
         carry on"
    )]
    Answered,
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
        images: Vec<ChatImage>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        files: Vec<ChatFile>,
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
    #[error("That is not one of your lines in this conversation.")]
    NotAChiefLine,
    #[error("That is not one of your lines in this conversation.")]
    NoSuchItem { item: String },
    #[error("Only the current session of a chief's chat goes back.")]
    NotCurrent,
    #[error("a turn is running here; stop it, then rewind")]
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
    pub usage: Usage,
}

/// What a conversation's model calls used, summed over it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub calls: Count,
    /// The calls whose provider told no usage.
    pub unreported: Count,
    pub input: Count,
    pub cached: Count,
    pub output: Count,
    pub reasoning: Count,
    pub ms: Count,
    /// Millionths of a dollar.
    pub micros: Count,
    /// The input of the last call.
    pub last_input: Count,
    pub summary: String,
}

/// A whole number that can pass 2^32, written as decimal text: the
/// agent's encoder writes a larger integer as a float.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Count(pub u64);

impl Serialize for Count {
    fn serialize<S: Serializer>(
        &self,
        to: S,
    ) -> Result<S::Ok, S::Error> {
        to.collect_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Count {
    fn deserialize<D: Deserializer<'de>>(
        from: D,
    ) -> Result<Self, D::Error> {
        String::deserialize(from)?.parse().map(Self).map_err(|_| {
            D::Error::custom(
                "a count is a whole number in decimal text",
            )
        })
    }
}

/// One item of a chat, as the client protocol's `Item`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatItem {
    pub id: String,
    /// In milliseconds.
    pub at: Count,
    pub kind: protocol::ItemKind,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// The name of the chief who said a chief's line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chief: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<ChatImage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<ChatFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rewindable: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<ChatTool>,
}

/// An image a chief's line carried, as the client protocol's
/// `AttachedImage`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatImage {
    /// The byte offset in the line's text where it sits; absent when
    /// it goes after the text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<u32>,
    pub id: Sha256,
}

/// A file a chief's line carried, as the client protocol's
/// `AttachedFile`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatFile {
    /// The byte offset in the line's text where it sits; absent when
    /// it goes after the text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<u32>,
    pub id: Sha256,
    pub name: String,
    pub size: u32,
}

/// A tool call's details, as the client protocol's `ItemTool`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatTool {
    pub name: String,
    pub summary: String,
    /// The exit code's 32 bits, so a negative code `c` is
    /// `c + 2^32`: TypeScript writes `c >>> 0` and Rust reads it
    /// back with `as i32`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ms: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change: Option<ToolChange>,
}

/// The change a tool call made to a file, as the client protocol's
/// `Change`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolChange {
    pub path: String,
    pub created: bool,
    pub added: u32,
    pub removed: u32,
    /// The lines of the changed stretch, with those around it.
    pub total: u32,
    /// `added` and `removed` in words.
    pub count: String,
    /// The first 200 lines of that stretch.
    pub lines: Vec<ChangeLine>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeLine {
    pub mark: LineMark,
    pub text: String,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum LineMark {
    Same,
    Added,
    Removed,
}

/// The event `chat` the agent publishes when a chat changes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatEvent {
    pub chat: ChatRef,
    pub epoch: u32,
    /// One more than the agent's last change before it, in this
    /// epoch.
    pub number: u32,
    pub ops: Vec<ChatOp>,
}

/// The chat an event is of, as the client protocol's `ChatRef`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatRef {
    /// The agent whose conversation it is.
    pub with: AgentNumber,
    /// The peer whose letters or questions it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<AgentNumber>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<protocol::ChatRefKind>,
    /// The number of an earlier session of the chief's chat.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub past: Option<u32>,
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
        usage: Usage,
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
    pub tokens: Count,
    pub title: String,
}

/// Reads the agent's whole system prompt as it would be sent now
/// (C14).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadPrompt;
from_client!(ReadPrompt, "read_prompt", Prompt);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prompt {
    /// In the order the prompt holds them, the empty ones left out.
    pub sections: Vec<PromptSection>,
}

/// A section of the prompt, as the client protocol's
/// `PromptSection`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptSection {
    pub name: String,
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
    /// The agent is not the Hand, which alone signs in.
    NotTheHand,
}

/// Reads the models the agent's listing holds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadModels;
from_client!(ReadModels, "read_models", ModelList);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelList {
    pub models: Vec<ListedModel>,
}

/// A model the listing holds, named as the catalogue names it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ListedModel {
    /// `provider::model`.
    pub name: String,
    pub title: String,
    /// Its context window, in tokens.
    pub context: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price: Option<Price>,
    /// The images it takes; absent when it takes none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub images: Option<ListedImages>,
    /// The efforts it can be asked for, weakest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub efforts: Vec<hamlet_protocol::Effort>,
    /// Whether its provider has a fast tier it can be asked for.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fast: bool,
}

/// The images a model takes, as its provider lists them: each limit
/// absent where the listing names none.
#[derive(
    Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize,
)]
pub struct ListedImages {
    /// The most bytes of one image.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_bytes: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_height: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_width: Option<u32>,
    /// The most images one line gives it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub per_line: Option<u32>,
}

/// Reads how many images one line may give the model the agent runs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImagesTaken;
from_client!(ImagesTaken, "images_taken", TakenImages);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TakenImages {
    /// Absent when the model takes none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub per_line: Option<u32>,
}

/// Asks `model` for a hello. It is neither admitted to a conversation
/// nor charged, and a repeat asks again.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Check {
    /// The model the agent runs when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}
from_client!(Check, "check", Checked);

/// The model's hello, its failure, or a refusal of its name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Checked {
    Answered {
        words: String,
        /// The models its endpoint lists, after a connection check.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        models: Option<u32>,
    },
    Failed {
        words: String,
    },
    Refused {
        words: String,
    },
}

/// Asks the Hand to reach the model service the realm's settings
/// name, as a connection check does. A repeat asks again.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckConnection;
from_client!(CheckConnection, "check_connection", Checked);

/// Reads the providers the Hand can sign in to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadProviders;
from_client!(ReadProviders, "read_providers", ProviderList);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProviderList {
    pub providers: Vec<hamlet_protocol::client::ProviderView>,
}

/// Reads what the agent knows: the character and the papers of its
/// own filesystem, and the paths it changed against its profile.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadPapers;
from_client!(
    ReadPapers,
    "read_papers",
    hamlet_protocol::client::Papers
);

/// Writes `text` to the file `path` under `memory/` in the agent's
/// filesystem, or removes it when `text` is empty, and saves it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditMemory {
    /// A path under `memory/`, starting with it.
    pub path: String,
    pub text: String,
}
from_client!(EditMemory, "edit_memory", ());
