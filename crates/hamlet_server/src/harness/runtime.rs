//! The runtime protocol: the messages the server and a harness
//! runtime send each other, one JSON object per line. The pi harness
//! validates them against `harnesses/pi/src/protocol-schema.ts`, the
//! JSON Schema of these types that `cargo test -p hamlet_server`
//! writes.

#![allow(clippy::large_enum_variant)]

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The runtime protocol version this server speaks.
pub const VERSION: u64 = 1;

/// The input is kept in the runtime's store; a repeated request
/// answers as the first did.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Admitted {
    /// What the harness said of an unanswered input.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "Value")]
    pub detail: Option<Value>,
    #[schemars(length(min = 1))]
    pub id: String,
    #[schemars(length(min = 1))]
    pub re: String,
    /// Why an unanswered input ended.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
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

/// Asks early whether a tool call may run, so a wait shows before the
/// call is kept as begun.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Allow {
    pub arguments: BTreeMap<String, Value>,
    #[schemars(length(min = 1))]
    pub conversation: String,
    #[schemars(length(min = 1))]
    pub id: String,
    /// The runtime's submission id of the input whose turn makes the
    /// call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "String")]
    pub input: Option<String>,
    #[schemars(length(min = 1))]
    pub key: String,
    #[schemars(length(min = 1))]
    pub tool: String,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
}

/// The call may be made with the model the server chose, and the
/// credential it needs.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Allowance {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "Credential")]
    pub credential: Option<Credential>,
    #[schemars(length(min = 1))]
    pub id: String,
    #[schemars(length(min = 1))]
    pub re: String,
    pub selection: Selection,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
}

/// The call may run.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Allowed {
    #[schemars(length(min = 1))]
    pub id: String,
    #[schemars(length(min = 1))]
    pub re: String,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
}

/// A wait for the operation `key` was answered, so asking again may
/// get further.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Answered {
    #[schemars(length(min = 1))]
    pub id: String,
    #[schemars(length(min = 1))]
    pub key: String,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
}

/// Asks before each model call.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Attempt {
    #[schemars(length(min = 1))]
    pub conversation: String,
    #[schemars(length(min = 1))]
    pub id: String,
    #[schemars(length(min = 1))]
    pub key: String,
    pub model: ModelRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "String")]
    pub task: Option<String>,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
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

/// A thing a runtime can do; the server refuses a runtime that lacks
/// one Hamlet needs.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub enum Capability {
    #[serde(rename = "operations")]
    Operations,
    #[serde(rename = "attempts")]
    Attempts,
    #[serde(rename = "admission")]
    Admission,
    #[serde(rename = "cancel")]
    Cancel,
    #[serde(rename = "recovery")]
    Recovery,
    #[serde(rename = "updates")]
    Updates,
    #[serde(rename = "history")]
    History,
}

/// The usage is charged, once however often it was reported.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Charged {
    #[schemars(length(min = 1))]
    pub id: String,
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

/// A secret the server hands over for one model call, held in memory
/// alone.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum Credential {
    #[serde(rename = "api_key")]
    ApiKey {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[serde(deserialize_with = "present")]
        #[schemars(with = "BTreeMap<String, String>")]
        env: Option<BTreeMap<String, String>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[serde(deserialize_with = "present")]
        #[schemars(with = "String")]
        key: Option<String>,
    },
    #[serde(rename = "oauth")]
    Oauth {
        access: String,
        expires: f64,
        extra: BTreeMap<String, Value>,
        refresh: String,
    },
}

/// A credential read or change, restricted to an authorized provider
/// scope.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct CredentialRequest {
    pub action: CredentialRequestAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "Credential")]
    pub credential: Option<Credential>,
    #[schemars(length(min = 1))]
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "String")]
    pub lease: Option<String>,
    pub scope: String,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub enum CredentialRequestAction {
    #[serde(rename = "read")]
    Read,
    #[serde(rename = "acquire")]
    Acquire,
    #[serde(rename = "commit")]
    Commit,
    #[serde(rename = "release")]
    Release,
    #[serde(rename = "end")]
    End,
}

/// The latest credential under the host's provider lease, or a
/// refusal.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct CredentialResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "Credential")]
    pub credential: Option<Credential>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "String")]
    pub error: Option<String>,
    #[schemars(length(min = 1))]
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "String")]
    pub lease: Option<String>,
    #[schemars(length(min = 1))]
    pub re: String,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
}

/// The server's note on a tool's run, shown to the model after its
/// output.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Diagnostic {
    pub code: String,
    pub message: String,
    pub severity: DiagnosticSeverity,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub enum DiagnosticSeverity {
    #[serde(rename = "error")]
    Error,
    #[serde(rename = "warn")]
    Warn,
    #[serde(rename = "info")]
    Info,
}

/// The reason a message could not be handled.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub enum ErrorCode {
    #[serde(rename = "malformed")]
    Malformed,
    #[serde(rename = "unknown_conversation")]
    UnknownConversation,
    #[serde(rename = "unsupported")]
    Unsupported,
    #[serde(rename = "failed")]
    Failed,
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

/// The runtime's first message: the versions it speaks, what it is
/// and what its store records.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Hello {
    pub adapter: String,
    pub capabilities: Vec<Capability>,
    pub harness: String,
    #[schemars(length(min = 1))]
    pub id: String,
    /// The runtime id its store records; null when there is no
    /// store.
    #[serde(deserialize_with = "Option::deserialize")]
    pub runtime: Option<String>,
    pub store: HelloStore,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
    #[schemars(inner(range(min = 1)))]
    pub versions: Vec<u64>,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub enum HelloStore {
    #[serde(rename = "created")]
    Created,
    #[serde(rename = "opened")]
    Opened,
    #[serde(rename = "missing")]
    Missing,
}

/// Asks for a page of a conversation's kept entries, newest last,
/// before the entry `before`.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct History {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
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
    #[serde(deserialize_with = "present")]
    #[schemars(with = "Vec<Block>")]
    pub blocks: Option<Vec<Block>>,
    /// The call a tool result answers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "String")]
    pub call: Option<String>,
    /// The server's account of the tool's run, such as its exit
    /// code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "BTreeMap<String, Value>")]
    pub details: Option<BTreeMap<String, Value>>,
    #[schemars(length(min = 1))]
    pub entry: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "bool")]
    pub error: Option<bool>,
    /// The words an assistant's answer failed with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "String")]
    pub failure: Option<String>,
    pub kind: ItemKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "Mark")]
    pub mark: Option<Mark>,
    /// How long the tool ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "u64")]
    pub ms: Option<u64>,
    /// The request id of the input or write that placed the entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "String")]
    pub request: Option<String>,
    /// Why an assistant's answer ended.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "String")]
    pub stop: Option<String>,
    /// A user's or a tool result's words.
    pub text: String,
    /// The tool a tool result answers for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "String")]
    pub tool: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
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

/// A message that could not be handled, in either direction; `re`
/// names it when its id was readable.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct ProtocolError {
    pub code: ErrorCode,
    #[schemars(length(min = 1))]
    pub id: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "String")]
    pub re: Option<String>,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
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
    #[serde(deserialize_with = "present")]
    #[schemars(with = "Partial")]
    pub partial: Option<Partial>,
    pub pending: u64,
    pub running: bool,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
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

/// A model by its provider and id, and the address it is served at
/// when that is not the provider's own.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct ModelRef {
    #[serde(rename = "baseUrl")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "String", length(min = 1))]
    pub base_url: Option<String>,
    #[schemars(length(min = 1))]
    pub id: String,
    #[schemars(length(min = 1))]
    pub provider: String,
}

/// Carries out a tool call under its operation key; the server
/// decides as it runs.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Operation {
    pub arguments: BTreeMap<String, Value>,
    #[schemars(length(min = 1))]
    pub conversation: String,
    #[schemars(length(min = 1))]
    pub id: String,
    /// The runtime's submission id of the input whose turn makes the
    /// call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "String")]
    pub input: Option<String>,
    #[schemars(length(min = 1))]
    pub key: String,
    #[schemars(length(min = 1))]
    pub tool: String,
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

/// The answer being written, before it is kept.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Partial {
    pub at: u64,
    pub blocks: Vec<Block>,
}

/// A model's price in dollars per million tokens.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Price {
    #[serde(rename = "cacheRead")]
    #[schemars(range(min = 0))]
    pub cache_read: f64,
    #[serde(rename = "cacheWrite")]
    #[schemars(range(min = 0))]
    pub cache_write: f64,
    #[schemars(range(min = 0))]
    pub input: f64,
    #[schemars(range(min = 0))]
    pub output: f64,
}

/// Asks for a conversation's prompt sections before each model call.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Prompt {
    #[schemars(length(min = 1))]
    pub conversation: String,
    #[schemars(length(min = 1))]
    pub id: String,
    /// The runtime's submission id of the input whose turn makes the
    /// call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "String")]
    pub input: Option<String>,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
}

/// A connection-owned sign-in's reply or cancellation.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct ProviderControl {
    pub cancel: bool,
    #[schemars(length(min = 1))]
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "String")]
    pub prompt: Option<String>,
    pub scope: String,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "String")]
    pub value: Option<String>,
}

/// A transient sign-in step, never a conversation entry.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct ProviderEvent {
    #[schemars(length(min = 1))]
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "String")]
    pub prompt: Option<String>,
    pub scope: String,
    pub step: BTreeMap<String, Value>,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
}

/// Provider work requested by the server, with no provider state
/// written by the runtime.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct ProviderRequest {
    pub action: ProviderRequestAction,
    #[serde(rename = "baseUrl")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "String")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "Credential")]
    pub credential: Option<Credential>,
    #[schemars(length(min = 1))]
    pub id: String,
    #[serde(rename = "installationId")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "String")]
    pub installation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "ProviderRequestMethod")]
    pub method: Option<ProviderRequestMethod>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "String")]
    pub model: Option<String>,
    pub models: Vec<BTreeMap<String, Value>>,
    pub provider: String,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub enum ProviderRequestAction {
    #[serde(rename = "describe")]
    Describe,
    #[serde(rename = "refresh")]
    Refresh,
    #[serde(rename = "hello")]
    Hello,
    #[serde(rename = "login")]
    Login,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub enum ProviderRequestMethod {
    #[serde(rename = "key")]
    Key,
    #[serde(rename = "subscription")]
    Subscription,
}

/// The result of provider work, for the server alone; the runtime
/// never records a returned credential.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct ProviderResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "Credential")]
    pub credential: Option<Credential>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "String")]
    pub error: Option<String>,
    #[schemars(length(min = 1))]
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "Vec<BTreeMap<String, Value>>")]
    pub models: Option<Vec<BTreeMap<String, Value>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "Vec<ProviderResultProviders>")]
    pub providers: Option<Vec<ProviderResultProviders>>,
    #[schemars(length(min = 1))]
    pub re: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "String")]
    pub text: Option<String>,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
}

/// A provider and whether it signs in with a key or a subscription.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct ProviderResultProviders {
    pub id: String,
    pub key: bool,
    pub name: String,
    pub subscription: bool,
}

/// The call may not run, in words for the agent.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Refusal {
    #[schemars(length(min = 1))]
    pub id: String,
    #[schemars(length(min = 1))]
    pub re: String,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
    pub words: String,
}

/// The server will not take the runtime on, in words for a chief; the
/// runtime exits.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Refused {
    #[schemars(length(min = 1))]
    pub id: String,
    #[schemars(length(min = 1))]
    pub re: String,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
    pub words: String,
}

/// The marked result of an operation, for the agent.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Result {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "BTreeMap<String, Value>")]
    pub details: Option<BTreeMap<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "Vec<Diagnostic>")]
    pub diagnostics: Option<Vec<Diagnostic>>,
    pub error: bool,
    #[schemars(length(min = 1))]
    pub id: String,
    #[schemars(length(min = 1))]
    pub re: String,
    pub text: String,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
}

/// A message a runtime sends the server.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum RuntimeMessage {
    #[serde(rename = "provider_result")]
    ProviderResult(ProviderResult),
    #[serde(rename = "provider_event")]
    ProviderEvent(ProviderEvent),
    #[serde(rename = "credential_request")]
    CredentialRequest(CredentialRequest),
    #[serde(rename = "hello")]
    Hello(Hello),
    #[serde(rename = "created")]
    Created(Created),
    #[serde(rename = "admitted")]
    Admitted(Admitted),
    #[serde(rename = "configured")]
    Configured(Configured),
    #[serde(rename = "cancelled")]
    Cancelled(Cancelled),
    #[serde(rename = "page")]
    Page(Page),
    #[serde(rename = "statuses")]
    Statuses(Statuses),
    #[serde(rename = "allow")]
    Allow(Allow),
    #[serde(rename = "operation")]
    Operation(Operation),
    #[serde(rename = "attempt")]
    Attempt(Attempt),
    #[serde(rename = "usage")]
    Usage(UsageReport),
    #[serde(rename = "scrub")]
    Scrub(Scrub),
    #[serde(rename = "prompt")]
    Prompt(Prompt),
    #[serde(rename = "kept")]
    Kept(Kept),
    #[serde(rename = "live")]
    Live(Live),
    #[serde(rename = "settled")]
    Settled(Settled),
    #[serde(rename = "yield")]
    Yield(Yield),
    #[serde(rename = "error")]
    Error(ProtocolError),
}

/// Asks to remove the server's known credentials from a value before
/// the runtime keeps or shows it.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Scrub {
    #[schemars(length(min = 1))]
    pub id: String,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
    pub value: Value,
}

/// The value with the server's known credentials removed.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Scrubbed {
    #[schemars(length(min = 1))]
    pub id: String,
    #[schemars(length(min = 1))]
    pub re: String,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
    pub value: Value,
}

/// A prompt section by name, rendered in order before each model
/// call.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Section {
    #[schemars(length(min = 1))]
    pub name: String,
    pub text: String,
}

/// A conversation's prompt sections, in order.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Sections {
    #[schemars(length(min = 1))]
    pub id: String,
    #[schemars(length(min = 1))]
    pub re: String,
    pub sections: Vec<Section>,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
}

/// The model one call is made with, as the server chose it; a size it
/// does not know is the model's own.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    #[serde(rename = "contextWindow")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "u64", range(min = 1))]
    pub context_window: Option<u64>,
    #[serde(rename = "maxTokens")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "u64", range(min = 1))]
    pub max_tokens: Option<u64>,
    pub model: ModelRef,
    pub price: Price,
}

/// A message the server sends a runtime.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum ServerMessage {
    #[serde(rename = "provider_request")]
    ProviderRequest(ProviderRequest),
    #[serde(rename = "provider_control")]
    ProviderControl(ProviderControl),
    #[serde(rename = "credential_result")]
    CredentialResult(CredentialResult),
    #[serde(rename = "welcome")]
    Welcome(Welcome),
    #[serde(rename = "tools")]
    Tools(Tools),
    #[serde(rename = "refused")]
    Refused(Refused),
    #[serde(rename = "create")]
    Create(Create),
    #[serde(rename = "submit")]
    Submit(Submit),
    #[serde(rename = "write")]
    Write(Write),
    #[serde(rename = "configure")]
    Configure(Configure),
    #[serde(rename = "fork")]
    Fork(Fork),
    #[serde(rename = "cancel")]
    Cancel(Cancel),
    #[serde(rename = "history")]
    History(History),
    #[serde(rename = "close")]
    Close(Close),
    #[serde(rename = "status")]
    Status(Status),
    #[serde(rename = "answered")]
    Answered(Answered),
    #[serde(rename = "allowed")]
    Allowed(Allowed),
    #[serde(rename = "waiting")]
    Waiting(Waiting),
    #[serde(rename = "refusal")]
    Refusal(Refusal),
    #[serde(rename = "result")]
    Result(Result),
    #[serde(rename = "allowance")]
    Allowance(Allowance),
    #[serde(rename = "stopped")]
    Stopped(Stopped),
    #[serde(rename = "charged")]
    Charged(Charged),
    #[serde(rename = "scrubbed")]
    Scrubbed(Scrubbed),
    #[serde(rename = "sections")]
    Sections(Sections),
    #[serde(rename = "verdict")]
    Verdict(Verdict),
    #[serde(rename = "error")]
    Error(ProtocolError),
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
    #[serde(deserialize_with = "present")]
    #[schemars(with = "Value")]
    pub detail: Option<Value>,
    #[schemars(length(min = 1))]
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
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
    #[serde(deserialize_with = "present")]
    #[schemars(with = "Mark")]
    pub notice: Option<Mark>,
    #[schemars(length(min = 1))]
    pub re: String,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
    pub words: String,
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
    #[serde(deserialize_with = "present")]
    #[schemars(with = "Mark")]
    pub mark: Option<Mark>,
    #[schemars(length(min = 1))]
    pub request: String,
    pub steer: bool,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "u64", range(min = 1))]
    pub window: Option<u64>,
}

/// A tool the server runs, as the model is offered it.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct ToolSpec {
    pub description: String,
    #[schemars(length(min = 1))]
    pub name: String,
    /// The JSON Schema of its arguments.
    pub parameters: BTreeMap<String, Value>,
    /// Whether a call cut off before its result was kept may run
    /// again; one that may not answers that it was cut off.
    pub replay: ToolSpecReplay,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub enum ToolSpecReplay {
    #[serde(rename = "safe")]
    Safe,
    #[serde(rename = "unsafe")]
    Unsafe,
}

/// Replaces the runtime's tool definitions before its next request.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Tools {
    #[schemars(length(min = 1))]
    pub id: String,
    pub tools: Vec<ToolSpec>,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
}

/// Tokens of one model call and its cost in dollars.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Usage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "f64", range(min = 0))]
    pub billed: Option<f64>,
    #[serde(rename = "cacheRead")]
    pub cache_read: u64,
    #[serde(rename = "cacheWrite")]
    pub cache_write: u64,
    #[schemars(range(min = 0))]
    pub cost: f64,
    #[serde(rename = "estimatedTokens")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "u64")]
    pub estimated_tokens: Option<u64>,
    #[serde(rename = "firstTokenMs")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "u64")]
    pub first_token_ms: Option<u64>,
    pub input: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "u64")]
    pub ms: Option<u64>,
    pub output: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "u64")]
    pub reasoning: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "String")]
    pub stop: Option<String>,
}

/// The usage of an allowed attempt, before its answer is kept.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct UsageReport {
    #[schemars(length(min = 1))]
    pub id: String,
    #[schemars(length(min = 1))]
    pub key: String,
    pub usage: Usage,
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
    #[serde(deserialize_with = "present")]
    #[schemars(with = "bool")]
    pub compact: Option<bool>,
    #[schemars(length(min = 1))]
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "Mark")]
    pub notice: Option<Mark>,
    #[schemars(length(min = 1))]
    pub re: String,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "String")]
    pub words: Option<String>,
}

/// The call waits for a chief; nothing is kept of it as a result.
/// Asked again once `key` is answered.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Waiting {
    #[schemars(length(min = 1))]
    pub id: String,
    #[schemars(length(min = 1))]
    pub key: String,
    #[schemars(length(min = 1))]
    pub re: String,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
    pub words: String,
}

/// The server takes the runtime on, with the tools it runs for the
/// runtime's conversations.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Welcome {
    #[schemars(length(min = 1))]
    pub id: String,
    #[schemars(length(min = 1))]
    pub re: String,
    pub tools: Vec<ToolSpec>,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
    #[schemars(extend("additionalProperties" = {"type": "integer", "format": "uint64", "minimum": 1}))]
    pub windows: BTreeMap<String, u64>,
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
    #[serde(deserialize_with = "present")]
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
    #[serde(deserialize_with = "present")]
    #[schemars(with = "bool")]
    pub history: Option<bool>,
    #[schemars(length(min = 1))]
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "String")]
    pub input: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "bool")]
    pub retried: Option<bool>,
    #[serde(rename = "summaryFailure")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "String")]
    pub summary_failure: Option<String>,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
}

/// A field that is present, so `null` is refused rather than read as
/// absent.
fn present<'de, D, T>(
    deserializer: D,
) -> std::result::Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}
