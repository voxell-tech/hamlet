//! The prompt's sections, and scrubbing credentials out of what is
//! kept.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::runtime::VERSION;

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
    #[serde(deserialize_with = "crate::runtime::present")]
    #[schemars(with = "String")]
    pub input: Option<String>,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
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
