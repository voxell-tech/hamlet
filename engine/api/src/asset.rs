//! The names of assets and of their types. The engine never looks
//! inside an asset: it is bytes and the name of its type.

use core::fmt;

use serde::{Deserialize, Serialize};

use crate::id::{IdError, is_name, string_id};

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
