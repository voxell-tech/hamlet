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

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Rewind {
    pub chat: ChatRef,
    pub id: RequestId,
    pub item: String,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct New {
    pub id: RequestId,
    pub to: AgentId,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Resume {
    pub id: RequestId,
    pub number: i64,
    pub to: AgentId,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "AgentId")]
    pub from: Option<AgentId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "ChatRefKind")]
    pub kind: Option<ChatRefKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "i64")]
    pub past: Option<i64>,
    /// The agent whose conversation it is.
    pub with: AgentId,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub enum ChatRefKind {
    #[serde(rename = "letters")]
    Letters,
    #[serde(rename = "questions")]
    Questions,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Turn {
    pub chat: ChatRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "i64")]
    pub number: Option<i64>,
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
    pub items: Vec<Item>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub last: Option<LastActivity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "i64")]
    pub number: Option<i64>,
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
        #[serde(deserialize_with = "Option::deserialize")]
        last: Option<LastActivity>,
        pending: u64,
        working: bool,
    },
    #[serde(rename = "usage")]
    Usage { usage: UsageView },
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
    /// The files a chief's line carried.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<AttachedFile>,
    pub id: String,
    /// The images a chief's line carried.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<AttachedImage>,
    pub kind: ItemKind,
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
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct ItemTool {
    #[serde(deserialize_with = "Option::deserialize")]
    pub change: Option<Change>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub exit: Option<i64>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub failure: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub ms: Option<u64>,
    pub name: String,
    pub summary: String,
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

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Past {
    pub began: i64,
    pub by: String,
    pub current: bool,
    pub messages: i64,
    pub modified: i64,
    pub number: i64,
    pub title: String,
    pub tokens: u64,
}
