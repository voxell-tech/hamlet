//! The entries a conversation keeps, as the protocol shows them.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Usage;

/// One kept entry of a conversation, as the server shows it.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Item {
    /// When the entry's message was made, in milliseconds since the
    /// epoch; 0 for an entry with no message.
    pub at: u64,
    /// An assistant's answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::present")]
    #[schemars(with = "Vec<Block>")]
    pub blocks: Option<Vec<Block>>,
    /// The call a tool result answers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::present")]
    #[schemars(with = "String")]
    pub call: Option<String>,
    /// The server's account of the tool's run, such as its exit
    /// code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::present")]
    #[schemars(with = "BTreeMap<String, Value>")]
    pub details: Option<BTreeMap<String, Value>>,
    #[schemars(length(min = 1))]
    pub entry: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::present")]
    #[schemars(with = "bool")]
    pub error: Option<bool>,
    /// The words an assistant's answer failed with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::present")]
    #[schemars(with = "String")]
    pub failure: Option<String>,
    pub kind: ItemKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::present")]
    #[schemars(with = "Mark")]
    pub mark: Option<Mark>,
    /// How long the tool ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::present")]
    #[schemars(with = "u64")]
    pub ms: Option<u64>,
    /// The request id of the input or write that placed the entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::present")]
    #[schemars(with = "String")]
    pub request: Option<String>,
    /// Why an assistant's answer ended.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::present")]
    #[schemars(with = "String")]
    pub stop: Option<String>,
    /// A user's or a tool result's words.
    pub text: String,
    /// The tool a tool result answers for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::present")]
    #[schemars(with = "String")]
    pub tool: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::present")]
    #[schemars(with = "Usage")]
    pub usage: Option<Usage>,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub enum ItemKind {
    #[serde(rename = "user")]
    User,
    #[serde(rename = "assistant")]
    Assistant,
    #[serde(rename = "tool_result")]
    ToolResult,
    #[serde(rename = "summary")]
    Summary,
    #[serde(rename = "other")]
    Other,
}

/// One block of an assistant's answer, in the order the answer gave
/// them.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum Block {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "thinking")]
    Thinking { thinking: String },
    #[serde(rename = "call")]
    Call {
        arguments: BTreeMap<String, Value>,
        id: String,
        name: String,
    },
}

/// Hamlet's own kind and data of an entry, such as a chief's line or
/// a notice, kept as the server gave them.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Mark {
    pub data: BTreeMap<String, Value>,
    #[schemars(length(min = 1))]
    pub kind: String,
}

/// The answer being written, before it is kept.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Partial {
    pub at: u64,
    pub blocks: Vec<Block>,
}
