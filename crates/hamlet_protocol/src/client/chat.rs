//! Watched conversations: what the chief says and the transcripts and
//! turns the server sends back.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::client::{
    AgentId, AttachedFile, AttachedImage, RequestId, UsageView,
};

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Say {
    /// The files the line carries, each sent before it. They are
    /// saved for the agent; its model is not given them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(length(max = 10000))]
    pub files: Vec<AttachedFile>,
    pub id: RequestId,
    /// The images the line carries, each sent before it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(length(max = 20))]
    pub images: Vec<AttachedImage>,
    pub text: String,
    pub to: AgentId,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Stop {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "AgentId")]
    pub agent: Option<AgentId>,
    pub id: RequestId,
}

/// Ends the hold a stop put on an agent, or on every agent the chief
/// reaches when `agent` is left out, so what waits takes its turns.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Resume {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "AgentId")]
    pub agent: Option<AgentId>,
    pub id: RequestId,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Rewind {
    pub chat: ChatRef,
    pub id: RequestId,
    pub item: String,
}

/// Asks the agent's conversation to summarize its earlier part now.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Compact {
    pub id: RequestId,
    pub to: AgentId,
}

/// Takes back a chief's line that is still queued.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Withdraw {
    pub chat: ChatRef,
    pub id: RequestId,
    /// The queued item's id.
    pub item: String,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Retry {
    pub id: RequestId,
    pub to: AgentId,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Open {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "String")]
    pub before: Option<String>,
    pub chat: ChatRef,
    pub id: RequestId,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Close {
    pub chat: ChatRef,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct ChatRef {
    /// The agent whose conversation it is.
    pub with: AgentId,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Turn {
    pub chat: ChatRef,
    #[serde(rename = "ops")]
    pub operations: Vec<TranscriptOperation>,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Transcript {
    pub chat: ChatRef,
    #[serde(deserialize_with = "Option::deserialize")]
    pub earlier: Option<String>,
    /// A chief's stop holds what waits: no queued turn starts until
    /// a chief's line or a resume.
    pub held: bool,
    pub items: Vec<Item>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub last: Option<LastActivity>,
    pub pending: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "RequestId")]
    pub to: Option<RequestId>,
    pub usage: UsageView,
    pub working: bool,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(tag = "op", deny_unknown_fields)]
#[expect(
    clippy::large_enum_variant,
    reason = "a wire type; a box would show in every pattern"
)]
pub enum TranscriptOperation {
    #[serde(rename = "add")]
    Add {
        item: Item,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[serde(deserialize_with = "crate::client::present")]
        #[schemars(with = "String")]
        replaces: Option<String>,
    },
    #[serde(rename = "append")]
    Append { item: String, text: String },
    #[serde(rename = "close")]
    Close {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[serde(deserialize_with = "crate::client::present")]
        #[schemars(with = "AgentId")]
        agent: Option<AgentId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[serde(deserialize_with = "crate::client::present")]
        #[schemars(with = "Option<Change>")]
        change: Option<Option<Change>>,
        exit: i64,
        #[serde(deserialize_with = "Option::deserialize")]
        failure: Option<String>,
        item: String,
        #[serde(deserialize_with = "Option::deserialize")]
        ms: Option<u64>,
    },
    #[serde(rename = "status")]
    Status {
        /// A chief's stop holds what waits.
        held: bool,
        #[serde(deserialize_with = "Option::deserialize")]
        last: Option<LastActivity>,
        pending: u64,
        working: bool,
    },
    #[serde(rename = "usage")]
    Usage { usage: UsageView },
    /// Removes an item, such as a queued line taken back.
    #[serde(rename = "remove")]
    Remove { item: String },
    #[serde(rename = "reset")]
    Reset {},
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Item {
    pub at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "String")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "String")]
    pub chief: Option<String>,
    /// What changed, for an `event` item.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "ItemEvent")]
    pub event: Option<ItemEvent>,
    /// The files a chief's line carried.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<AttachedFile>,
    /// The agent a message from another agent is from, or the hire a
    /// `merged` item brought in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "AgentId")]
    pub from: Option<AgentId>,
    pub id: String,
    /// The images a chief's line carried.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<AttachedImage>,
    pub kind: ItemKind,
    /// What a message from another agent is, beside its `author`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "ItemMessage")]
    pub message: Option<ItemMessage>,
    /// A line kept for the agent that its turn has not taken up yet.
    /// Queued items are the last of a transcript, in the order their
    /// turns will run.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub queued: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "bool")]
    pub rewindable: Option<bool>,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "ItemTool")]
    pub tool: Option<ItemTool>,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub enum ItemKind {
    #[serde(rename = "user")]
    User,
    #[serde(rename = "assistant")]
    Assistant,
    #[serde(rename = "reasoning")]
    Reasoning,
    #[serde(rename = "tool")]
    Tool,
    #[serde(rename = "notice")]
    Notice,
    #[serde(rename = "error")]
    Error,
    #[serde(rename = "answered")]
    Answered,
    /// The summary that stands for the earlier part of the
    /// conversation in the model's context.
    #[serde(rename = "summary")]
    Summary,
    /// Another hire's conversation, brought in by a merge; `from`
    /// names it.
    #[serde(rename = "merged")]
    Merged,
    /// A change to the agent, shown between its turns for people
    /// alone; `event` says what changed.
    #[serde(rename = "event")]
    Event,
}

/// What a message from another agent is.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    JsonSchema,
)]
pub enum ItemMessage {
    #[serde(rename = "notification")]
    Notification,
    #[serde(rename = "request")]
    Request,
    #[serde(rename = "response")]
    Response,
    /// Why a request or a worker has no answer.
    #[serde(rename = "error-response")]
    ErrorResponse,
}

/// A change to an agent its conversation shows between turns. The
/// model never reads it.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum ItemEvent {
    /// The model it runs, or how hard it thinks, changed by `by`.
    #[serde(rename = "model")]
    Model {
        by: String,
        from: ItemModel,
        to: ItemModel,
    },
    /// Stopped by `by`, with `waiting` inputs held in its queue.
    #[serde(rename = "stopped")]
    Stopped { by: String, waiting: u64 },
    /// The hold of a stop ended by `by`.
    #[serde(rename = "resumed")]
    Resumed { by: String },
    /// A dream brought what it learned home; `summary` is the first
    /// line of the dream's answer.
    #[serde(rename = "dreamed")]
    Dreamed { summary: String },
    /// A scholar of the city, `scholar`, was made of what it became.
    #[serde(rename = "scholar")]
    Scholar { scholar: String },
    /// The name it is shown by now.
    #[serde(rename = "renamed")]
    Renamed { name: String },
    /// The provider's fast tier turned on or off.
    #[serde(rename = "fast")]
    Fast { on: bool },
}

/// A model an agent runs and how hard it thinks, as an event names
/// them.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct ItemModel {
    /// The catalogue's name of the model, `provider::id`.
    pub model: String,
    /// Its effort, or `null` where it takes the realm's.
    #[serde(deserialize_with = "Option::deserialize")]
    pub effort: Option<crate::Effort>,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct ItemTool {
    /// The agent a society call sent to or started, once it ended.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "AgentId")]
    pub agent: Option<AgentId>,
    pub call: ToolCall,
    #[serde(deserialize_with = "Option::deserialize")]
    pub change: Option<Change>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub exit: Option<i64>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub failure: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub ms: Option<u64>,
    pub name: String,
    /// The call's arguments as JSON, shortened.
    pub summary: String,
}

/// What a tool call's row shows, read from its tool and arguments.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum ToolCall {
    /// A notification to another agent, by the address it was sent
    /// to.
    #[serde(rename = "notification")]
    Notification { to: String, text: String },
    /// A request to another agent, by the address it was sent to.
    #[serde(rename = "request")]
    Request { to: String, text: String },
    /// A worker started on `task`.
    #[serde(rename = "worker")]
    Worker { task: String },
    /// A shell command.
    #[serde(rename = "command")]
    Command { command: String },
    /// A call on one file or folder.
    #[serde(rename = "path")]
    Path { path: String },
    /// Any other call, shown by its `summary`.
    #[serde(rename = "other")]
    Other,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Change {
    pub added: u64,
    pub count: String,
    pub created: bool,
    #[schemars(length(max = 200))]
    pub lines: Vec<ChangeLines>,
    pub path: String,
    pub removed: u64,
    pub total: u64,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct ChangeLines {
    pub mark: ChangeLinesMark,
    #[schemars(length(max = 240))]
    pub text: String,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub enum ChangeLinesMark {
    #[serde(rename = " ")]
    Space,
    #[serde(rename = "+")]
    Plus,
    #[serde(rename = "-")]
    Minus,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct LastActivity {
    pub at: u64,
    pub what: String,
}
