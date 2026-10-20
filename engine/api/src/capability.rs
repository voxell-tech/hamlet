//! What a plugin may ask the person for. Powers that are never
//! granted, such as keys or running processes, have no name here,
//! so no manifest can ask for them.

use serde::{Deserialize, Serialize};

/// A kind of access a plugin asks for, by its stable name.
#[derive(
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Debug,
    Serialize,
    Deserialize,
)]
pub enum Capability {
    /// Fetching from one host, the ask's target.
    #[serde(rename = "net.fetch")]
    NetFetch,
    /// Reading assets of one type, the ask's target.
    #[serde(rename = "assets.read")]
    AssetRead,
    /// Calling one service of another plugin, the ask's target
    /// (`plugin.id/service`).
    #[serde(rename = "services.call")]
    ServiceCall,
}

impl Capability {
    /// Its name, as a manifest writes it.
    pub fn name(self) -> &'static str {
        match self {
            Self::NetFetch => "net.fetch",
            Self::AssetRead => "assets.read",
            Self::ServiceCall => "services.call",
        }
    }
}

/// One thing a plugin asks for, and why, in its own words.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Ask {
    pub capability: Capability,
    /// What the access is to: a host, a type name, a service.
    pub target: String,
    pub reason: String,
}
