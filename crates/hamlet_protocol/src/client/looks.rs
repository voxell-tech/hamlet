//! What agents and hamlets wear: official assets of the Agent
//! Bazaar's server, each named `id@version`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::client::Address;

/// What an agent or a hamlet wears, each part an official asset as
/// `id@version`, such as `agent-skin-sakura@1.0.0`.
#[derive(
    Clone,
    Debug,
    Default,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Looks {
    /// A short animation it plays now and then.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "String")]
    pub emote: Option<String>,
    /// A shape that replaces its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "String")]
    pub mesh: Option<String>,
    /// Colours over its own shape.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "String")]
    pub skin: Option<String>,
}

impl Looks {
    /// Whether it wears nothing.
    pub fn is_bare(&self) -> bool {
        self.emote.is_none()
            && self.mesh.is_none()
            && self.skin.is_none()
    }
}

/// The one whose looks an edit changes.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(untagged, deny_unknown_fields)]
pub enum Wearer {
    Agent { agent: Address },
    Hamlet { hamlet: String },
}

/// The parts of looks an edit changes: an asset puts it on, `null`
/// takes it off, and a part left out stays as it is.
#[derive(
    Clone,
    Debug,
    Default,
    PartialEq,
    Serialize,
    Deserialize,
    JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct LooksSet {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "Option<String>")]
    pub emote: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "Option<String>")]
    pub mesh: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "Option<String>")]
    pub skin: Option<Option<String>>,
}
