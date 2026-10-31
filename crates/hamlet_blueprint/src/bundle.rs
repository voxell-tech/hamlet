//! What a realm's host chooses: a ruleset and the files of the assets
//! it adds, as one file.

use std::collections::BTreeMap;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::graph::{Ruleset, to_ron};

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Bundle {
    pub ruleset: Ruleset,
    /// Each asset's file, by its path in the catalogue, written as
    /// base64 so the bundle stays text.
    #[serde(serialize_with = "encode", deserialize_with = "decode")]
    pub files: BTreeMap<String, Vec<u8>>,
}

impl Bundle {
    pub fn to_ron(&self) -> Result<String, ron::Error> {
        to_ron(self)
    }

    pub fn from_ron(
        text: &str,
    ) -> Result<Self, ron::error::SpannedError> {
        ron::from_str(text)
    }
}

fn encode<S: Serializer>(
    files: &BTreeMap<String, Vec<u8>>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    files
        .iter()
        .map(|(path, bytes)| (path, STANDARD.encode(bytes)))
        .collect::<BTreeMap<_, _>>()
        .serialize(serializer)
}

fn decode<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<String, Vec<u8>>, D::Error> {
    BTreeMap::<String, String>::deserialize(deserializer)?
        .into_iter()
        .map(|(path, text)| {
            STANDARD
                .decode(text)
                .map(|bytes| (path, bytes))
                .map_err(serde::de::Error::custom)
        })
        .collect()
}
