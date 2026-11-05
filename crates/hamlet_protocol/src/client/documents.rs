//! Edits and reads of the chief's documents.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::client::{
    AgentId, Endpoint, Listed, Past, Price, RequestId, ScholarId,
    Skill,
};

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(tag = "doc", deny_unknown_fields)]
pub enum Edit {
    #[serde(rename = "charter")]
    Charter {
        id: RequestId,
        #[serde(deserialize_with = "Option::deserialize")]
        text: Option<String>,
    },
    /// A scholar's character, as a commit on its `main`.
    #[serde(rename = "character")]
    Character {
        id: RequestId,
        scholar: ScholarId,
        text: String,
    },
    /// A memory file of a scholar's `main` or a hire's lodge, by
    /// its path under `memory/`; empty text removes it.
    #[serde(rename = "memory")]
    Memory {
        id: RequestId,
        of: Keeper,
        path: String,
        text: String,
    },
    #[serde(rename = "limits")]
    Limits(EditLimits),
    #[serde(rename = "settings")]
    Settings { id: RequestId, set: EditSettingsSet },
    #[serde(rename = "catalogue")]
    Catalogue {
        id: RequestId,
        model: String,
        set: EditCatalogueSet,
    },
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(untagged, deny_unknown_fields)]
pub enum EditLimits {
    Set {
        /// The hamlet whose limits these are, or the realm's when
        /// absent.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[serde(deserialize_with = "crate::client::present")]
        #[schemars(with = "String")]
        hamlet: Option<String>,
        id: RequestId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[serde(deserialize_with = "crate::client::present")]
        #[schemars(with = "String")]
        reason: Option<String>,
        set: LimitsEdit,
    },
    Whole {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[serde(deserialize_with = "crate::client::present")]
        #[schemars(with = "String")]
        hamlet: Option<String>,
        id: RequestId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[serde(deserialize_with = "crate::client::present")]
        #[schemars(with = "String")]
        reason: Option<String>,
        whole: std::collections::BTreeMap<String, serde_json::Value>,
    },
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct LimitsEdit {
    pub key: String,
    pub section: String,
    pub value: serde_json::Value,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct EditSettingsSet {
    #[serde(rename = "allowNet")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "bool")]
    pub allow_net: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "String", length(max = 4000))]
    pub character: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "Option<crate::Effort>")]
    pub effort: Option<Option<crate::Effort>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "Vec<Endpoint>")]
    pub endpoints: Option<Vec<Endpoint>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "String")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "crate::client::Newcomers")]
    pub newcomers: Option<crate::client::Newcomers>,
    #[serde(rename = "reflectEvery")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "i64")]
    pub reflect_every: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "Option<EditSettingsSetWorkspace>")]
    pub workspace: Option<Option<EditSettingsSetWorkspace>>,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct EditSettingsSetWorkspace {
    pub token: String,
    pub url: String,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct EditCatalogueSet {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "bool")]
    pub allowed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "String")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "Option<Price>")]
    pub price: Option<Option<Price>>,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(tag = "what", deny_unknown_fields)]
pub enum Read {
    #[serde(rename = "prompt")]
    Prompt { agent: AgentId, id: RequestId },
    #[serde(rename = "history")]
    History { agent: AgentId, id: RequestId },
    #[serde(rename = "models")]
    Models {
        id: RequestId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[serde(deserialize_with = "crate::client::present")]
        #[schemars(with = "bool")]
        refresh: Option<bool>,
    },
    #[serde(rename = "papers")]
    Papers {
        of: Keeper,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[serde(deserialize_with = "crate::client::present")]
        #[schemars(with = "bool")]
        full: Option<bool>,
        id: RequestId,
    },
    #[serde(rename = "charter")]
    Charter { id: RequestId },
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(untagged, deny_unknown_fields)]
pub enum ReadResult {
    Prompt { prompt: String, to: RequestId },
    History { history: Vec<Past>, to: RequestId },
    Models { models: Vec<Listed>, to: RequestId },
    Papers { papers: Papers, to: RequestId },
    Charter { charter: Charter, to: RequestId },
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Charter {
    pub own: bool,
    pub text: String,
}

/// Whose knowledge an edit or a read is of: a scholar's `main`, or an
/// agent's lodge laid over its scholar.
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
#[serde(rename_all = "lowercase")]
pub enum Keeper {
    Scholar(ScholarId),
    Agent(AgentId),
}

/// A scholar's knowledge, or what an agent sees of it in its lodge.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Papers {
    pub character: String,
    /// The paths of the files the agent's lodge changed, added or
    /// removed against its scholar; empty for a scholar.
    pub learned: Vec<String>,
    pub memory: Vec<MemoryFile>,
    pub skills: Vec<Skill>,
}

/// A file under `memory/`.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct MemoryFile {
    /// Its front matter's `description:` line.
    pub description: String,
    /// Its path under `memory/`.
    pub path: String,
    pub text: String,
}
