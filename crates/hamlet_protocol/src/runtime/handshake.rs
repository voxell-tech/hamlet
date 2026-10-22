//! How a runtime introduces itself and the server takes it on or
//! refuses it.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::runtime::{ToolSpec, VERSION};

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
