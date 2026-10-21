//! Model calls: the model chosen, its price, and the usage charged.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Credential, VERSION};

/// A model by its provider and id, and the address it is served at
/// when that is not the provider's own.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct ModelRef {
    #[serde(rename = "baseUrl")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::present")]
    #[schemars(with = "String", length(min = 1))]
    pub base_url: Option<String>,
    #[schemars(length(min = 1))]
    pub id: String,
    #[schemars(length(min = 1))]
    pub provider: String,
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

/// The model one call is made with, as the server chose it; a size it
/// does not know is the model's own.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    #[serde(rename = "contextWindow")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::present")]
    #[schemars(with = "u64", range(min = 1))]
    pub context_window: Option<u64>,
    #[serde(rename = "maxTokens")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::present")]
    #[schemars(with = "u64", range(min = 1))]
    pub max_tokens: Option<u64>,
    pub model: ModelRef,
    pub price: Price,
}

/// Tokens of one model call and its cost in dollars.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Usage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::present")]
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
    #[serde(deserialize_with = "crate::present")]
    #[schemars(with = "u64")]
    pub estimated_tokens: Option<u64>,
    #[serde(rename = "firstTokenMs")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::present")]
    #[schemars(with = "u64")]
    pub first_token_ms: Option<u64>,
    pub input: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::present")]
    #[schemars(with = "u64")]
    pub ms: Option<u64>,
    pub output: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::present")]
    #[schemars(with = "u64")]
    pub reasoning: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::present")]
    #[schemars(with = "String")]
    pub stop: Option<String>,
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
    #[serde(deserialize_with = "crate::present")]
    #[schemars(with = "String")]
    pub task: Option<String>,
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
    #[serde(deserialize_with = "crate::present")]
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
