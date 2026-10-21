//! Creating, feeding, forking and closing a runtime's conversations,
//! and what it reports of them.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Item, Mark, ModelRef, Partial, VERSION};

/// Creates a conversation once per `key`; asking again answers with
/// the same conversation.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Create {
    /// The agent it is for, as `<hamlet>:<number>`.
    #[schemars(regex(pattern = r"^[^:]+:[0-9]+$"))]
    pub agent: String,
    pub errand: bool,
    #[schemars(length(min = 1))]
    pub id: String,
    #[schemars(length(min = 1))]
    pub key: String,
    pub model: ModelRef,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
}

/// The conversation created or forked, by the id the runtime made for
/// it.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Created {
    #[schemars(length(min = 1))]
    pub conversation: String,
    #[schemars(length(min = 1))]
    pub id: String,
    #[schemars(length(min = 1))]
    pub re: String,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
}

/// An input for a conversation, admitted once by its request id.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Submit {
    pub content: String,
    #[schemars(length(min = 1))]
    pub conversation: String,
    #[schemars(length(min = 1))]
    pub id: String,
    /// Hamlet's kind and data for the entry that places it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::present")]
    #[schemars(with = "Mark")]
    pub mark: Option<Mark>,
    #[schemars(length(min = 1))]
    pub request: String,
    pub steer: bool,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::present")]
    #[schemars(with = "u64", range(min = 1))]
    pub window: Option<u64>,
}

/// An entry only people see, written at once even while a turn runs;
/// admitted once by its request id.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Write {
    /// Written at a boundary, so the inputs queued behind a run that
    /// ended without them are placed after it and run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::present")]
    #[schemars(with = "bool")]
    pub boundary: Option<bool>,
    #[schemars(length(min = 1))]
    pub conversation: String,
    #[schemars(length(min = 1))]
    pub id: String,
    pub mark: Mark,
    #[schemars(length(min = 1))]
    pub request: String,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
}

/// Sets the tools a conversation offers from its next request, kept
/// in the runtime's store; asking again sets the same.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Configure {
    #[schemars(length(min = 1))]
    pub conversation: String,
    #[schemars(length(min = 1))]
    pub id: String,
    pub tools: ConfigureTools,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
}

/// Exactly the `only` tools, or every tool but the `remove` ones.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(untagged, deny_unknown_fields)]
pub enum ConfigureTools {
    Only {
        #[schemars(inner(length(min = 1)))]
        only: Vec<String>,
    },
    Remove {
        #[schemars(inner(length(min = 1)))]
        remove: Vec<String>,
    },
}

/// The conversation's tools are set.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Configured {
    #[schemars(length(min = 1))]
    pub id: String,
    #[schemars(length(min = 1))]
    pub re: String,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
}

/// Copies a conversation up to the entry `at`, once per `key`;
/// answered with the copy as `created`.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Fork {
    #[schemars(length(min = 1))]
    pub at: String,
    #[schemars(length(min = 1))]
    pub conversation: String,
    #[schemars(length(min = 1))]
    pub id: String,
    #[schemars(length(min = 1))]
    pub key: String,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
}

/// The input is kept in the runtime's store; a repeated request
/// answers as the first did.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Admitted {
    /// What the harness said of an unanswered input.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::present")]
    #[schemars(with = "Value")]
    pub detail: Option<Value>,
    #[schemars(length(min = 1))]
    pub id: String,
    #[schemars(length(min = 1))]
    pub re: String,
    /// Why an unanswered input ended.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::present")]
    #[schemars(with = "String")]
    pub reason: Option<String>,
    pub status: AdmittedStatus,
    pub submission: String,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub enum AdmittedStatus {
    #[serde(rename = "queued")]
    Queued,
    #[serde(rename = "placed")]
    Placed,
    #[serde(rename = "done")]
    Done,
    #[serde(rename = "unanswered")]
    Unanswered,
}

/// Stops a conversation's running turn and drops its queue.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Cancel {
    #[schemars(length(min = 1))]
    pub conversation: String,
    #[schemars(length(min = 1))]
    pub id: String,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
}

/// The conversation's turn is stopped and its queue dropped.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Cancelled {
    #[schemars(length(min = 1))]
    pub id: String,
    #[schemars(length(min = 1))]
    pub re: String,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
}

/// Asks for a page of a conversation's kept entries, newest last,
/// before the entry `before`.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct History {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::present")]
    #[schemars(with = "String")]
    pub before: Option<String>,
    #[schemars(length(min = 1))]
    pub conversation: String,
    #[schemars(length(min = 1))]
    pub id: String,
    #[schemars(range(min = 1, max = 200))]
    pub limit: u64,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
}

/// A page of kept entries, and whether earlier ones remain.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Page {
    pub earlier: bool,
    #[schemars(length(min = 1))]
    pub id: String,
    pub items: Vec<Item>,
    #[schemars(length(min = 1))]
    pub re: String,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
}

/// Stops the runtime in order: its running work is signalled and
/// waited on, then it exits.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Close {
    #[schemars(length(min = 1))]
    pub id: String,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
}

/// Asks what each of the runtime's conversations is doing.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Status {
    #[schemars(length(min = 1))]
    pub id: String,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
}

/// Whether each of the runtime's conversations is running.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Statuses {
    pub conversations: Vec<StatusesConversations>,
    #[schemars(length(min = 1))]
    pub id: String,
    #[schemars(length(min = 1))]
    pub re: String,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
}

/// Whether one conversation is running.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct StatusesConversations {
    #[schemars(length(min = 1))]
    pub conversation: String,
    pub running: bool,
}

/// A conversation's state, sent as it changes: whether it runs, its
/// queued inputs and the answer being written.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Live {
    #[schemars(length(min = 1))]
    pub conversation: String,
    #[schemars(length(min = 1))]
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::present")]
    #[schemars(with = "Partial")]
    pub partial: Option<Partial>,
    pub pending: u64,
    pub running: bool,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
}

/// An input or write that ended, by its request id.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Settled {
    #[schemars(length(min = 1))]
    pub conversation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::present")]
    #[schemars(with = "Value")]
    pub detail: Option<Value>,
    #[schemars(length(min = 1))]
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::present")]
    #[schemars(with = "String")]
    pub reason: Option<String>,
    #[schemars(length(min = 1))]
    pub request: String,
    pub status: SettledStatus,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub enum SettledStatus {
    #[serde(rename = "done")]
    Done,
    #[serde(rename = "unanswered")]
    Unanswered,
}

/// An entry the runtime kept.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Kept {
    #[schemars(length(min = 1))]
    pub conversation: String,
    #[schemars(length(min = 1))]
    pub id: String,
    pub item: Item,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
}

/// The call may not be made, in words of what ran out. With `notice`,
/// the runtime keeps it and ends the turn as stopped rather than
/// failed.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Stopped {
    #[schemars(length(min = 1))]
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::present")]
    #[schemars(with = "Mark")]
    pub notice: Option<Mark>,
    #[schemars(length(min = 1))]
    pub re: String,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
    pub words: String,
}

/// Asks whether a final answer stands, before the run ends with it.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Yield {
    pub answer: Item,
    #[schemars(length(min = 1))]
    pub conversation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::present")]
    #[schemars(with = "bool")]
    pub history: Option<bool>,
    #[schemars(length(min = 1))]
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::present")]
    #[schemars(with = "String")]
    pub input: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::present")]
    #[schemars(with = "bool")]
    pub retried: Option<bool>,
    #[serde(rename = "summaryFailure")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::present")]
    #[schemars(with = "String")]
    pub summary_failure: Option<String>,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
}

/// The answer stands, or fails with `words`, after which the run
/// ends; `notice` is kept with the failure.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Verdict {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::present")]
    #[schemars(with = "bool")]
    pub compact: Option<bool>,
    #[schemars(length(min = 1))]
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::present")]
    #[schemars(with = "Mark")]
    pub notice: Option<Mark>,
    #[schemars(length(min = 1))]
    pub re: String,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::present")]
    #[schemars(with = "String")]
    pub words: Option<String>,
}
