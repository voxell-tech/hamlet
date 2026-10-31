//! The assets a book's graphs may put down, by name: the built-in
//! ones and those the book adds.

use std::collections::BTreeMap;

use bevy_math::{Quat, Vec3};
use serde::{Deserialize, Serialize};

/// The assets a book adds, by name, each in place of a built-in one
/// of the same name.
#[derive(
    Clone, PartialEq, Debug, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Catalogue(pub BTreeMap<String, Asset>);

/// What draws an asset, the colour of each of its material slots,
/// and the points named on it that other assets are put at.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Asset {
    pub shape: Shape,
    #[serde(default)]
    pub slots: BTreeMap<String, Paint>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sockets: BTreeMap<String, Socket>,
}

/// A point named on an asset, where another is put, such as where the
/// lamp hangs.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct Socket {
    /// Where it is in the box around the asset, from -0.5 to 0.5 on
    /// each axis, so it stays on the asset however it is sized.
    pub at: Vec3,
    pub turn: Quat,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub enum Shape {
    /// A rounded box filling the placement, with one slot,
    /// [`BODY`].
    Box,
    /// A glTF file in the folder beside the book, by its path there.
    Mesh(String),
}

/// What colours a material slot.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub enum Paint {
    /// A colour of the world's palette, by name, so the asset takes
    /// the hamlet's look and its grade by day and night.
    Palette(String),
    /// The colours and textures the asset's file gives it.
    Own,
}

/// The slot of a [`Shape::Box`].
pub const BODY: &str = "body";

/// The built-in assets: boxes, each in a colour of the palette.
const BUILTIN: &[(&str, &str)] = &[
    ("box", "plaster"),
    ("wall_plaster", "plaster"),
    ("post", "timber"),
    ("post_timber", "timber"),
    ("beam_timber", "timber_dark"),
    ("frame_timber", "timber_dark"),
    ("door", "timber_dark"),
    ("stone_base", "limestone"),
    ("stone_chimney", "limestone"),
    ("sill_stone", "limestone"),
    ("roof_tiles", "terracotta"),
    ("roof_slate", "slate_roof"),
    ("window", "slate_roof"),
    ("flower_box", "moss"),
];

impl Catalogue {
    /// The asset named `name`: the book's, else the built-in one.
    pub fn get(&self, name: &str) -> Option<Asset> {
        if let Some(asset) = self.0.get(name) {
            return Some(asset.clone());
        }
        let (_, colour) =
            BUILTIN.iter().find(|(builtin, _)| *builtin == name)?;
        Some(Asset {
            shape: Shape::Box,
            slots: BTreeMap::from([(
                BODY.to_owned(),
                Paint::Palette((*colour).to_owned()),
            )]),
            sockets: BTreeMap::new(),
        })
    }

    /// Every asset's name, the built-in ones with the book's.
    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = BUILTIN
            .iter()
            .map(|(name, _)| (*name).to_owned())
            .chain(self.0.keys().cloned())
            .collect();
        names.sort();
        names.dedup();
        names
    }

    /// Every socket's name on the book's assets, each once.
    pub fn socket_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .0
            .values()
            .flat_map(|asset| asset.sockets.keys().cloned())
            .collect();
        names.sort();
        names.dedup();
        names
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}
