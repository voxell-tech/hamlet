//! What agents and hamlets wear (docs/looks.md): the formats of the
//! official assets, which the Agent Bazaar sells, the realm draws and
//! the baker in `hamlet-server/assets-src` makes, and how the bazaar
//! tells the realm what is worn.
//!
//! The bazaar makes two types of asset in the engine's store, and the
//! realm reads them:
//!
//! - [`WORN`], one asset named [`WORN_NAME`]: a [`Worn`] in JSON,
//!   what each agent and hamlet wears.
//! - [`FILE`], one asset per part of the file of an asset worn, named
//!   [`part_name`]. A file is cut into parts of [`FILE_PART`] bytes
//!   at most, as one asset of the store holds less than a file may.
//!
//! The bazaar writes the parts of a file before the [`Worn`] that
//! names it, and drops parts no [`Worn`] names. The realm holds the
//! [`Worn`] asset, so it hears each change.

pub mod wardrobe;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The type of the asset that says what is worn, and its version.
pub const WORN: &str = "looks.worn";
pub const WORN_VERSION: u32 = 1;

/// The name of the one [`WORN`] asset.
pub const WORN_NAME: &str = "worn";

/// The type of an asset holding a part of a file worn, and its
/// version.
pub const FILE: &str = "looks.file";
pub const FILE_VERSION: u32 = 1;

/// The most bytes of a file one [`FILE`] asset holds, within the
/// store's `asset_bytes`.
pub const FILE_PART: usize = 448 * 1024;

/// The event the bazaar sends to have an agent's robot play the emote
/// it wears now, its payload the agent's name as a `String`, and the
/// payload's type.
pub const PLAY: &str = "hamlet.bazaar/emote";
pub const PLAY_PAYLOAD: &str = "looks.emote";

/// What each agent and hamlet wears.
#[derive(
    Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize,
)]
pub struct Worn {
    /// By the agent's name.
    pub agents: BTreeMap<String, Outfit>,
    /// By the hamlet's name on disk.
    pub hamlets: BTreeMap<String, Outfit>,
    /// How many parts each file worn is in, by its [`Worn::file`]
    /// name.
    pub parts: BTreeMap<String, u32>,
}

/// What one agent or hamlet wears, each the [`Worn::file`] name of an
/// owned asset.
#[derive(
    Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize,
)]
pub struct Outfit {
    pub skin: Option<String>,
    pub mesh: Option<String>,
    pub emote: Option<String>,
}

impl Worn {
    /// The name a file is known by: its asset's id and version, so a
    /// new version is a new file.
    pub fn file(id: &str, version: &str) -> String {
        format!("{id}@{version}")
    }

    /// Every file worn, each once.
    pub fn files(&self) -> impl Iterator<Item = &str> {
        let mut all = self
            .agents
            .values()
            .chain(self.hamlets.values())
            .flat_map(|outfit| {
                [&outfit.skin, &outfit.mesh, &outfit.emote]
                    .into_iter()
                    .flatten()
            })
            .map(String::as_str)
            .collect::<Vec<_>>();
        all.sort_unstable();
        all.dedup();
        all.into_iter()
    }

    pub fn read(bytes: &[u8]) -> serde_json::Result<Self> {
        serde_json::from_slice(bytes)
    }

    pub fn bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("a Worn is plain data")
    }
}

/// The name of part `at` of the file `file`.
pub fn part_name(file: &str, at: u32) -> String {
    format!("{file}#{at}")
}

/// How many parts a file of `bytes` is cut into.
pub fn parts(bytes: usize) -> u32 {
    bytes.div_ceil(FILE_PART).max(1) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_are_named_by_id_and_version_and_listed_once() {
        let skin = Worn::file("agent-skin-sakura", "1.0.0");
        let mut worn = Worn::default();
        for name in ["ada", "bo"] {
            worn.agents.insert(
                name.into(),
                Outfit {
                    skin: Some(skin.clone()),
                    ..Outfit::default()
                },
            );
        }
        assert_eq!(skin, "agent-skin-sakura@1.0.0");
        assert_eq!(worn.files().collect::<Vec<_>>(), [skin.as_str()]);
        assert_eq!(Worn::read(&worn.bytes()).unwrap(), worn);
        assert_eq!(part_name(&skin, 2), "agent-skin-sakura@1.0.0#2");
        assert_eq!(parts(0), 1);
        assert_eq!(parts(FILE_PART), 1);
        assert_eq!(parts(FILE_PART + 1), 2);
    }
}
