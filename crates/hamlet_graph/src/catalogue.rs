//! The assets a book's graphs may put down, by name: the built-in
//! ones and those the book adds.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};

use bevy_math::{Quat, Vec3};
use serde::de::Error;
use serde::{Deserialize, Deserializer, Serialize};

use crate::graph::{Graph, Setting, Spread};
use crate::value::Constant;

/// The assets a book adds, by name, each in place of a built-in one
/// of the same name.
#[derive(
    Clone, PartialEq, Debug, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Catalogue(pub BTreeMap<String, Asset>);

/// What draws an asset, its own size, the colour of each of its
/// material slots, and the points named on it that other assets are
/// put at.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Asset {
    pub shape: Shape,
    /// The box around it in metres, which a scale of one draws it
    /// at: a metre on each side for a built-in shape, and as it was
    /// made for a mesh.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub size: Vec3,
    #[serde(default)]
    pub slots: BTreeMap<String, Paint>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sockets: BTreeMap<String, Socket>,
}

fn one() -> Vec3 {
    Vec3::ONE
}

fn is_one(size: &Vec3) -> bool {
    *size == Vec3::ONE
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

/// How an asset is drawn. Each built-in shape has one slot,
/// [`BODY`], and stands on its transform, its box above the frame.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub enum Shape {
    /// A rounded box filling the transform.
    Box,
    /// A triangle across x, its peak at the top `peak` along x from
    /// the middle (from -0.5 to 0.5), as thick as the transform is
    /// along z: a gable's end. Two with their peaks at an end meet
    /// under a ridge that is not in the middle.
    Wedge { peak: f32 },
    /// Lapped roof tiles over the transform's frame, from its eave
    /// along x at +z, as wide as the transform, to its ridge at -z,
    /// `taper` times as wide. A hip's end has a taper of 0.
    Tiles { taper: f32 },
    /// A glTF file in the folder beside the book, by its path there.
    Mesh(#[serde(deserialize_with = "mesh_path")] String),
}

impl Shape {
    /// Whether `path` is a file below the folder it is in: not empty,
    /// not absolute and without `..`.
    pub fn is_safe_path(path: &str) -> bool {
        !path.is_empty()
            && Path::new(path)
                .components()
                .all(|part| matches!(part, Component::Normal(_)))
    }
}

fn mesh_path<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<String, D::Error> {
    let path = String::deserialize(deserializer)?;
    if Shape::is_safe_path(&path) {
        Ok(path)
    } else {
        Err(D::Error::custom(format!(
            "{path:?} is not a path inside the folder"
        )))
    }
}

/// What colours a material slot.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub enum Paint {
    /// A colour of the world's palette, by name, so the asset takes
    /// the hamlet's colours and their grade by day and night.
    Palette(String),
    /// The colours and textures the asset's file gives it.
    Own,
}

/// The slot of a built-in shape.
pub const BODY: &str = "body";

const BOX: Shape = Shape::Box;
const WEDGE: Shape = Shape::Wedge { peak: 0.0 };
const HALF: Shape = Shape::Wedge { peak: 0.5 };

/// The built-in assets, each a shape in a colour of the palette.
const BUILTIN: &[(&str, Shape, &str)] = &[
    ("box", BOX, "plaster"),
    ("wall_plaster", BOX, "plaster"),
    ("wall_ochre", BOX, "ochre"),
    ("wall_whitewash", BOX, "whitewash"),
    ("wall_stone", BOX, "limestone"),
    ("gable_plaster", WEDGE, "plaster"),
    ("gable_ochre", WEDGE, "ochre"),
    ("gable_whitewash", WEDGE, "whitewash"),
    ("gable_stone", WEDGE, "limestone"),
    ("gable_plaster_half", HALF, "plaster"),
    ("gable_ochre_half", HALF, "ochre"),
    ("gable_whitewash_half", HALF, "whitewash"),
    ("gable_stone_half", HALF, "limestone"),
    ("plinth", BOX, "limestone_shade"),
    ("quoin", BOX, "limestone_shade"),
    ("flagstone", BOX, "limestone"),
    ("jamb_stone", BOX, "limestone"),
    ("chimney_cap", BOX, "limestone_shade"),
    ("door_leaf", BOX, "livery"),
    ("shutter", BOX, "livery"),
    ("pane", BOX, "pane"),
    ("ridge_cap", BOX, "roof"),
    ("tiles", Shape::Tiles { taper: 1.0 }, "roof"),
    ("tiles_hip", Shape::Tiles { taper: 0.3 }, "roof"),
    ("tiles_hip_end", Shape::Tiles { taper: 0.0 }, "roof"),
    ("post", BOX, "timber"),
    ("post_timber", BOX, "timber"),
    ("beam_timber", BOX, "timber_dark"),
    ("frame_timber", BOX, "timber_dark"),
    ("door", BOX, "timber_dark"),
    ("stone_base", BOX, "limestone"),
    ("stone_chimney", BOX, "limestone"),
    ("sill_stone", BOX, "limestone"),
    ("roof_tiles", BOX, "terracotta"),
    ("roof_slate", BOX, "slate_roof"),
    ("window", BOX, "slate_roof"),
    ("flower_box", BOX, "moss"),
];

impl Catalogue {
    /// The asset named `name`: the book's, else the built-in one.
    pub fn get(&self, name: &str) -> Option<Asset> {
        if let Some(asset) = self.0.get(name) {
            return Some(asset.clone());
        }
        let (_, shape, colour) =
            BUILTIN.iter().find(|(builtin, ..)| *builtin == name)?;
        Some(Asset {
            shape: shape.clone(),
            size: Vec3::ONE,
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
            .map(|(name, ..)| (*name).to_owned())
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

    /// The book's assets that `graphs` name, as a choice they are set
    /// to or may roll, or an input's default.
    pub fn named_in<'a>(
        &self,
        graphs: impl IntoIterator<Item = &'a Graph>,
    ) -> Catalogue {
        let mut named = BTreeSet::new();
        for graph in graphs {
            for setting in graph
                .nodes
                .iter()
                .flat_map(|node| node.settings.values())
            {
                match setting {
                    Setting::Fixed(Constant::Choice(choice)) => {
                        named.insert(choice.as_str());
                    }
                    Setting::Random(roll) => {
                        if let Spread::Weights(weights) = &roll.spread
                        {
                            named.extend(
                                weights.iter().map(|(choice, _)| {
                                    choice.as_str()
                                }),
                            );
                        }
                    }
                    _ => {}
                }
            }
            for input in &graph.inputs {
                if let Some(Constant::Choice(choice)) = &input.default
                {
                    named.insert(choice.as_str());
                }
            }
        }
        Catalogue(
            self.0
                .iter()
                .filter(|(name, _)| named.contains(name.as_str()))
                .map(|(name, asset)| (name.clone(), asset.clone()))
                .collect(),
        )
    }

    pub fn to_ron(&self) -> Result<String, ron::Error> {
        crate::graph::to_ron(self)
    }

    pub fn from_ron(
        text: &str,
    ) -> Result<Self, ron::error::SpannedError> {
        ron::from_str(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mesh_path_outside_the_folder_does_not_read() {
        for path in ["", "/etc/passwd", "../up.glb", "a/../../up.glb"]
        {
            let text = format!("{{\"x\": (shape: Mesh({path:?}))}}");
            assert!(Catalogue::from_ron(&text).is_err(), "{path}");
        }
        let text = "{\"x\": (shape: Mesh(\"meshes/x.glb\"))}";
        assert!(Catalogue::from_ron(text).is_ok());
    }
}
