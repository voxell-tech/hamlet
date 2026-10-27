//! The tools a conversation may offer, and the server's say on each
//! call.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::runtime::VERSION;

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
    /// Which end of a result past the runtime's output limits it
    /// keeps; without it, the head.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::runtime::present")]
    #[schemars(with = "ToolSpecRetain")]
    pub retain: Option<ToolSpecRetain>,
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

#[derive(
    Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub enum ToolSpecRetain {
    #[serde(rename = "head")]
    Head,
    #[serde(rename = "tail")]
    Tail,
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
    #[serde(deserialize_with = "crate::runtime::present")]
    #[schemars(with = "String")]
    pub input: Option<String>,
    #[schemars(length(min = 1))]
    pub key: String,
    #[schemars(length(min = 1))]
    pub tool: String,
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
    #[serde(deserialize_with = "crate::runtime::present")]
    #[schemars(with = "String")]
    pub input: Option<String>,
    #[schemars(length(min = 1))]
    pub key: String,
    #[schemars(length(min = 1))]
    pub tool: String,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
}

/// The marked result of an operation, for the agent.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Result {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::runtime::present")]
    #[schemars(with = "BTreeMap<String, Value>")]
    pub details: Option<BTreeMap<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::runtime::present")]
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
