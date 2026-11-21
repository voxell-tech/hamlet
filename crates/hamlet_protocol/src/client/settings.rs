//! Settings, model providers, checks and sign-in.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::client::RequestId;

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct SettingsView {
    #[serde(rename = "allowNet")]
    pub allow_net: bool,
    /// The character a profile made blank starts from.
    pub character: String,
    #[serde(deserialize_with = "Option::deserialize")]
    pub effort: Option<crate::Effort>,
    pub endpoints: Vec<SettingsViewEndpoints>,
    pub link: Link,
    /// The MCP servers the realm lists (C36).
    pub mcp: crate::client::McpView,
    pub model: String,
    pub newcomers: Newcomers,
    #[serde(rename = "reflectEvery")]
    pub reflect_every: i64,
    pub unset: bool,
    pub workspace: SettingsViewWorkspace,
}

/// Who lets a profile an agent made into the city. One a chief makes
/// is always let in.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    JsonSchema,
)]
pub enum Newcomers {
    /// Nobody: it moves in at once.
    #[serde(rename = "open")]
    Open,
    /// A chief: it waits at the harbor until a chief welcomes it.
    #[default]
    #[serde(rename = "chief")]
    Chief,
    /// The Hand, for the chiefs, who can undo its verdict.
    #[serde(rename = "hand")]
    Hand,
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
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "SettingsViewWorkspaceMove")]
    pub moving: Option<SettingsViewWorkspaceMove>,
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
    /// What the provider last told of its limits; none before it was
    /// asked, and for a provider that keeps none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "ProviderLimits")]
    pub limits: Option<ProviderLimits>,
    pub methods: Vec<LoginMethod>,
    pub name: String,
}

/// How much of a provider's plan or key is used, as it last told it.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct ProviderLimits {
    /// When the provider told it, in milliseconds since the epoch.
    pub checked: i64,
    /// The plan's name, where the provider tells it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "String")]
    pub plan: Option<String>,
    pub state: LimitsState,
    pub windows: Vec<LimitWindow>,
}

/// Whether the provider lets the plan or key be used.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    JsonSchema,
)]
pub enum LimitsState {
    /// It answers.
    #[serde(rename = "ok")]
    Ok,
    /// It answers and says a limit is near.
    #[serde(rename = "warning")]
    Warning,
    /// It refused, for a limit, when the limits were told.
    #[serde(rename = "refused")]
    Refused,
    /// It tells nothing of its limits.
    #[serde(rename = "unknown")]
    Unknown,
}

/// One limit a provider keeps over a span of time, or over money.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct LimitWindow {
    /// The same window across reads, such as `five_hour`.
    pub id: String,
    /// How long the window lasts, in minutes, where it is a span of
    /// time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "i64")]
    pub length: Option<i64>,
    /// What is left and what the cap is, where it is money.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "LimitMoney")]
    pub money: Option<LimitMoney>,
    /// Its name, such as "Session" or "Weekly · Fable".
    pub name: String,
    /// When it resets, in milliseconds since the epoch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "i64")]
    pub resets: Option<i64>,
    /// How much of it is used, from 0 to 100.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "f64", range(min = 0.0, max = 100.0))]
    pub used: Option<f64>,
}

/// Money left under a cap, in US dollars.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct LimitMoney {
    pub cap: f64,
    pub left: f64,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Listed {
    #[serde(deserialize_with = "Option::deserialize")]
    pub context: Option<i64>,
    /// The effort the model thinks at unless asked otherwise; none
    /// while unknown.
    #[serde(rename = "defaultEffort")]
    #[serde(deserialize_with = "Option::deserialize")]
    pub default_effort: Option<crate::Effort>,
    /// The efforts the model can be asked for, weakest first; none
    /// are listed while they are unknown.
    pub efforts: Vec<crate::Effort>,
    /// Whether the model's provider has a fast tier it can be asked
    /// for.
    pub fast: bool,
    /// What the model takes of images; none when it takes no images.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "crate::client::ImageInput")]
    pub images: Option<crate::client::ImageInput>,
    pub name: String,
    pub note: String,
    #[serde(deserialize_with = "Option::deserialize")]
    pub price: Option<Price>,
    pub priced: ListedPriced,
    /// What its provider calls it, such as `Claude Opus 4.8`; empty
    /// when the provider gives no name.
    pub title: String,
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

impl ListedPriced {
    /// Whether a chief can set the model's price and images: unless
    /// its provider lists its price (S30).
    pub fn settable(&self) -> bool {
        *self != Self::Listed
    }
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
        id: RequestId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[serde(deserialize_with = "crate::client::present")]
        #[schemars(with = "String")]
        model: Option<String>,
    },
    #[serde(rename = "connection")]
    Connection { id: RequestId },
    /// Asks each signed-in provider how much of its limits is used,
    /// unless it told so in the last minute.
    #[serde(rename = "limits")]
    Limits { id: RequestId },
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct MoveWorktrees {
    pub id: RequestId,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(untagged, deny_unknown_fields)]
pub enum Login {
    Start {
        id: RequestId,
        start: LoginStart,
    },
    Reply {
        id: RequestId,
        reply: LoginReply,
    },
    Cancel {
        cancel: String,
        id: RequestId,
    },
    /// Signs out of the provider `forget` names, or removes its key.
    Forget {
        forget: String,
        id: RequestId,
    },
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct LoginStart {
    pub method: LoginMethod,
    pub provider: String,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct LoginReply {
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
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "RequestId")]
    pub to: Option<RequestId>,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(untagged, deny_unknown_fields)]
pub enum LoginStep {
    Url {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[serde(deserialize_with = "crate::client::present")]
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
