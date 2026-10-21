//! Settings, model providers, checks and sign-in.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::RequestId;

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct SettingsView {
    #[serde(rename = "allowNet")]
    pub allow_net: bool,
    pub endpoints: Vec<SettingsViewEndpoints>,
    pub link: Link,
    pub model: String,
    #[serde(rename = "reflectEvery")]
    pub reflect_every: i64,
    pub unset: bool,
    pub workspace: SettingsViewWorkspace,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct SettingsViewEndpoints {
    #[serde(rename = "baseUrl")]
    pub base_url: String,
    pub key: bool,
    pub name: String,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct SettingsViewWorkspace {
    #[serde(rename = "move")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::present")]
    #[schemars(with = "SettingsViewWorkspaceMove")]
    pub move_: Option<SettingsViewWorkspaceMove>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub running: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub url: Option<String>,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct SettingsViewWorkspaceMove {
    pub done: i64,
    pub of: i64,
    pub state: SettingsViewWorkspaceMoveState,
    pub text: String,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub enum SettingsViewWorkspaceMoveState {
    #[serde(rename = "moving")]
    Moving,
    #[serde(rename = "moved")]
    Moved,
    #[serde(rename = "failed")]
    Failed,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Endpoint {
    #[serde(rename = "baseUrl")]
    pub base_url: String,
    pub name: String,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Link {
    #[serde(deserialize_with = "Option::deserialize")]
    pub models: Option<i64>,
    pub state: LinkState,
    #[serde(deserialize_with = "Option::deserialize")]
    pub words: Option<String>,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub enum LinkState {
    #[serde(rename = "untested")]
    Untested,
    #[serde(rename = "testing")]
    Testing,
    #[serde(rename = "connected")]
    Connected,
    #[serde(rename = "failed")]
    Failed,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct ProviderView {
    pub configured: bool,
    pub id: String,
    pub label: String,
    pub methods: Vec<LoginMethod>,
    pub name: String,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Listed {
    pub allowed: bool,
    #[serde(deserialize_with = "Option::deserialize")]
    pub context: Option<i64>,
    pub name: String,
    pub note: String,
    #[serde(deserialize_with = "Option::deserialize")]
    pub price: Option<Price>,
    pub priced: ListedPriced,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub enum ListedPriced {
    #[serde(rename = "unknown")]
    Unknown,
    #[serde(rename = "listed")]
    Listed,
    #[serde(rename = "assigned")]
    Assigned,
    #[serde(rename = "yours")]
    Yours,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Price {
    pub input: f64,
    pub output: f64,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(tag = "what", deny_unknown_fields)]
pub enum Check {
    #[serde(rename = "model")]
    Model {
        #[schemars(extend("format" = "uuid"))]
        id: RequestId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[serde(deserialize_with = "crate::present")]
        #[schemars(with = "String")]
        model: Option<String>,
    },
    #[serde(rename = "connection")]
    Connection {
        #[schemars(extend("format" = "uuid"))]
        id: RequestId,
    },
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct MoveHouses {
    #[schemars(extend("format" = "uuid"))]
    pub id: RequestId,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(untagged, deny_unknown_fields)]
pub enum Login {
    Start {
        #[schemars(extend("format" = "uuid"))]
        id: RequestId,
        start: LoginStartStart,
    },
    Reply {
        #[schemars(extend("format" = "uuid"))]
        id: RequestId,
        reply: LoginReplyReply,
    },
    Cancel {
        cancel: String,
        #[schemars(extend("format" = "uuid"))]
        id: RequestId,
    },
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct LoginStartStart {
    pub method: LoginMethod,
    pub provider: String,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct LoginReplyReply {
    pub login: String,
    pub value: String,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub enum LoginMethod {
    #[serde(rename = "key")]
    Key,
    #[serde(rename = "subscription")]
    Subscription,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct LoginProgress {
    pub login: String,
    pub step: LoginStep,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::present")]
    #[schemars(with = "RequestId", extend("format" = "uuid"))]
    pub to: Option<RequestId>,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(untagged, deny_unknown_fields)]
pub enum LoginStep {
    Url {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[serde(deserialize_with = "crate::present")]
        #[schemars(with = "String")]
        instructions: Option<String>,
        url: String,
    },
    Code {
        code: String,
    },
    Prompt {
        prompt: String,
        secret: bool,
    },
    Progress {
        progress: String,
    },
    Done {
        done: String,
    },
    Failed {
        failed: String,
    },
}
