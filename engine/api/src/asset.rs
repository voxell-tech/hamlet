//! Assets: bytes and the name of their type. The engine never reads
//! inside an asset; the plugin that publishes a type says what its
//! bytes mean.

use core::fmt;

use serde::{Deserialize, Serialize};

use crate::id::{IdError, PluginId, is_name, string_id};

/// An asset, as the store numbers it.
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
pub struct AssetId(pub u64);

string_id!(
    /// The name of a type of asset, as the plugin that publishes it
    /// gives it: `weather.forecast`.
    TypeName,
    "type name",
    is_name
);

/// A type of asset at a version, as a manifest names one it makes or
/// uses.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetType {
    pub name: TypeName,
    pub version: u32,
}

/// The types of asset a plugin makes and those of others it uses, as
/// its manifest's `[assets]` names them.
#[derive(
    Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize,
)]
#[serde(deny_unknown_fields)]
pub struct AssetTypes {
    #[serde(default)]
    pub makes: Vec<AssetType>,
    #[serde(default)]
    pub uses: Vec<AssetType>,
}

/// What the store keeps of an asset besides its bytes.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct AssetInfo {
    pub id: AssetId,
    pub kind: AssetType,
    /// The plugin that made it, the only one that may change it.
    pub owner: PluginId,
    pub name: String,
    /// One at first, and one more with each change.
    pub version: u64,
}

/// An asset and its bytes.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Asset {
    pub info: AssetInfo,
    pub bytes: Vec<u8>,
}
