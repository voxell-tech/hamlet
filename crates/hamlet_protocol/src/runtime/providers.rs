//! Provider work and the credentials the server hands over for it.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::runtime::VERSION;

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
    #[serde(deserialize_with = "crate::runtime::present")]
    #[schemars(with = "String")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::runtime::present")]
    #[schemars(with = "Credential")]
    pub credential: Option<Credential>,
    #[schemars(length(min = 1))]
    pub id: String,
    #[serde(rename = "installationId")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::runtime::present")]
    #[schemars(with = "String")]
    pub installation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::runtime::present")]
    #[schemars(with = "ProviderRequestMethod")]
    pub method: Option<ProviderRequestMethod>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::runtime::present")]
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
    #[serde(deserialize_with = "crate::runtime::present")]
    #[schemars(with = "String")]
    pub prompt: Option<String>,
    pub scope: String,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::runtime::present")]
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
    #[serde(deserialize_with = "crate::runtime::present")]
    #[schemars(with = "String")]
    pub prompt: Option<String>,
    pub scope: String,
    pub step: BTreeMap<String, Value>,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
}

/// The result of provider work, for the server alone; the runtime
/// never records a returned credential.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct ProviderResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::runtime::present")]
    #[schemars(with = "Credential")]
    pub credential: Option<Credential>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::runtime::present")]
    #[schemars(with = "String")]
    pub error: Option<String>,
    #[schemars(length(min = 1))]
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::runtime::present")]
    #[schemars(with = "Vec<BTreeMap<String, Value>>")]
    pub models: Option<Vec<BTreeMap<String, Value>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::runtime::present")]
    #[schemars(with = "Vec<ProviderResultProviders>")]
    pub providers: Option<Vec<ProviderResultProviders>>,
    #[schemars(length(min = 1))]
    pub re: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::runtime::present")]
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
        #[serde(deserialize_with = "crate::runtime::present")]
        #[schemars(with = "BTreeMap<String, String>")]
        env: Option<BTreeMap<String, String>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[serde(deserialize_with = "crate::runtime::present")]
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
    #[serde(deserialize_with = "crate::runtime::present")]
    #[schemars(with = "Credential")]
    pub credential: Option<Credential>,
    #[schemars(length(min = 1))]
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::runtime::present")]
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
    #[serde(deserialize_with = "crate::runtime::present")]
    #[schemars(with = "Credential")]
    pub credential: Option<Credential>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::runtime::present")]
    #[schemars(with = "String")]
    pub error: Option<String>,
    #[schemars(length(min = 1))]
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::runtime::present")]
    #[schemars(with = "String")]
    pub lease: Option<String>,
    #[schemars(length(min = 1))]
    pub re: String,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
}
