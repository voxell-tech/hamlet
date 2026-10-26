//! Edits and reads of the chief's documents.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::client::{
    Address, Bought, Endpoint, Listed, LooksSet, PackableTool,
    PackagePart, Packing, Past, Price, RequestId, Skill, Wearer,
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
    #[serde(rename = "character")]
    Character {
        agent: Address,
        id: RequestId,
        text: String,
    },
    #[serde(rename = "notes")]
    Notes {
        agent: Address,
        id: RequestId,
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
    /// Changes what an agent or a hamlet wears.
    #[serde(rename = "looks")]
    Looks {
        id: RequestId,
        set: LooksSet,
        wearer: Wearer,
    },
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(untagged, deny_unknown_fields)]
pub enum EditLimits {
    Set {
        hamlet: String,
        id: RequestId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[serde(deserialize_with = "crate::client::present")]
        #[schemars(with = "String")]
        reason: Option<String>,
        set: LimitsEdit,
    },
    Whole {
        hamlet: String,
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
    #[schemars(with = "Vec<Endpoint>")]
    pub endpoints: Option<Vec<Endpoint>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "String")]
    pub model: Option<String>,
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
    Prompt { agent: Address, id: RequestId },
    #[serde(rename = "history")]
    History { agent: Address, id: RequestId },
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
        agent: Address,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[serde(deserialize_with = "crate::client::present")]
        #[schemars(with = "bool")]
        full: Option<bool>,
        id: RequestId,
    },
    #[serde(rename = "charter")]
    Charter { id: RequestId },
    #[serde(rename = "bought")]
    Bought { id: RequestId },
    #[serde(rename = "tools")]
    Tools { id: RequestId },
    #[serde(rename = "package")]
    Package { id: RequestId, of: Packing },
    /// The home's OpenRouter key sealed with a Hamlet account's data
    /// key, as [`crate::vault::DataKey::seal`] seals it, to be kept
    /// by the bazaar's server.
    #[serde(rename = "sealed")]
    Sealed {
        /// The data key, as [`crate::vault::DataKey::to_base64`]
        /// writes it.
        #[serde(rename = "dataKey")]
        data_key: String,
        id: RequestId,
    },
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(untagged, deny_unknown_fields)]
pub enum ReadResult {
    Prompt {
        prompt: String,
        to: RequestId,
    },
    History {
        history: Vec<Past>,
        to: RequestId,
    },
    Models {
        models: Vec<Listed>,
        to: RequestId,
    },
    Papers {
        papers: Papers,
        to: RequestId,
    },
    Charter {
        charter: Charter,
        to: RequestId,
    },
    Bought {
        bought: Vec<Bought>,
        to: RequestId,
    },
    Tools {
        tools: Vec<PackableTool>,
        to: RequestId,
    },
    Package {
        package: PackagePart,
        to: RequestId,
    },
    /// The home's OpenRouter key sealed, or `null` when the home has
    /// none.
    Sealed {
        #[serde(deserialize_with = "Option::deserialize")]
        sealed: Option<String>,
        to: RequestId,
    },
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Charter {
    pub own: bool,
    pub text: String,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Papers {
    pub character: String,
    pub notes: String,
    pub skills: Vec<Skill>,
}
