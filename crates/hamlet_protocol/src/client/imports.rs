//! Conversations brought in from another program as a new session of
//! a chief's chat (C35).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::client::{AgentId, FileId, RequestId};

/// Brings in the conversation sent as the file `file`, an
/// [`Imported`] as JSON, as a new session of the chief's chat with
/// `to`.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Import {
    pub file: FileId,
    pub id: RequestId,
    pub to: AgentId,
}

/// A conversation from another program, as the client read it.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Imported {
    /// When it began, in milliseconds since the epoch.
    pub began: u64,
    /// Its first message and its newest ones, in order, with the
    /// tool calls between them.
    #[schemars(length(max = 4000))]
    pub entries: Vec<ImportedEntry>,
    /// The messages between the first and the newest that were left
    /// out.
    pub left_out: u64,
    /// The model that answered, as the source names it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "String", length(min = 1, max = 200))]
    pub model: Option<String>,
    /// The source's own id of the conversation.
    #[schemars(length(min = 1, max = 200))]
    pub session: String,
    pub source: ImportSource,
    #[schemars(length(max = 200))]
    pub title: String,
}

impl Imported {
    /// The messages kept besides the first.
    pub const NEWEST: usize = 200;
    /// The entries, tool calls included.
    pub const MOST_ENTRIES: usize = 4000;
    /// The bytes of the file an [`Import`] names.
    pub const MOST_BYTES: u64 = 16 << 20;
}

/// The program a conversation is brought in from.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum ImportSource {
    ClaudeCode,
    Codex,
    Pi,
    Chatgpt,
    Claude,
}

impl ImportSource {
    /// The program's name as a person reads it.
    pub fn name(self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::Codex => "Codex",
            Self::Pi => "pi",
            Self::Chatgpt => "ChatGPT",
            Self::Claude => "Claude",
        }
    }

    /// The name a command line takes.
    pub fn word(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
            Self::Pi => "pi",
            Self::Chatgpt => "chatgpt",
            Self::Claude => "claude",
        }
    }
}

/// One entry of an [`Imported`] conversation. Times are milliseconds
/// since the epoch.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum ImportedEntry {
    /// The person's message.
    User { at: u64, text: String },
    /// The model's answer, in words.
    Assistant { at: u64, text: String },
    /// A tool call the source made, shown to people and never given
    /// to the model.
    Step {
        at: u64,
        /// The call's input, shortened.
        #[schemars(length(max = 200))]
        summary: String,
        #[schemars(length(min = 1, max = 200))]
        tool: String,
    },
}
