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

/// How an asset is drawn. Each built-in shape but [`Shape::Builtin`]
/// has one slot, [`BODY`], and stands on its transform, its box above
/// the frame.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub enum Shape {
    /// A rounded box filling the transform.
    Box,
    /// A box filling the transform, its edges bevelled `bevel` metres
    /// deep and its bottom darkened by `ao`, from 0 to 1.
    Rounded { bevel: f32, ao: f32 },
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
    /// Drawn by Hamlet's own code for the asset's name, about the
    /// point its transform puts it at, with the slots that name has.
    Builtin,
    /// A point the game reads from what a graph puts down, as where a
    /// house's lantern hangs. Nothing is drawn there in the realm.
    Mark,
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
    /// A colour of the palette times a number: darker below one,
    /// lighter above.
    Shade(String, f32),
    /// The colours and textures the asset's file gives it.
    Own,
}

/// The slot of a built-in shape.
pub const BODY: &str = "body";

const BOX: Shape = Shape::Box;
const WEDGE: Shape = Shape::Wedge { peak: 0.0 };
const HALF: Shape = Shape::Wedge { peak: 0.5 };
const BUILT: Shape = Shape::Builtin;
const MARK: Shape = Shape::Mark;

const fn rounded(bevel: f32, ao: f32) -> Shape {
    Shape::Rounded { bevel, ao }
}

const ONE: Vec3 = Vec3::ONE;

/// The palette colour of a slot, by name, and the shade it is taken
/// at.
type Slot = (&'static str, &'static str, f32);

/// The built-in assets: each one's shape, size and the palette colour
/// of each of its slots. Those a house is made of may take the
/// colours each house picks for itself: `wall`, `upper_wall`, `roof`,
/// `drift` and `livery`. A [`Shape::Builtin`] is drawn about the
/// point it is put at: a window, a knob or a lantern's lamp at its
/// middle, the rest at the middle of their foot.
const BUILTIN: &[(&str, Shape, Vec3, &[Slot])] = &[
    ("box", BOX, ONE, &[(BODY, "plaster", 1.0)]),
    ("wall_plaster", BOX, ONE, &[(BODY, "plaster", 1.0)]),
    ("wall_ochre", BOX, ONE, &[(BODY, "ochre", 1.0)]),
    ("wall_whitewash", BOX, ONE, &[(BODY, "whitewash", 1.0)]),
    ("wall_stone", BOX, ONE, &[(BODY, "limestone", 1.0)]),
    ("gable_plaster", WEDGE, ONE, &[(BODY, "plaster", 1.0)]),
    ("gable_ochre", WEDGE, ONE, &[(BODY, "ochre", 1.0)]),
    ("gable_whitewash", WEDGE, ONE, &[(BODY, "whitewash", 1.0)]),
    ("gable_stone", WEDGE, ONE, &[(BODY, "limestone", 1.0)]),
    ("gable_plaster_half", HALF, ONE, &[(BODY, "plaster", 1.0)]),
    ("gable_ochre_half", HALF, ONE, &[(BODY, "ochre", 1.0)]),
    (
        "gable_whitewash_half",
        HALF,
        ONE,
        &[(BODY, "whitewash", 1.0)],
    ),
    ("gable_stone_half", HALF, ONE, &[(BODY, "limestone", 1.0)]),
    (
        "plinth",
        rounded(0.05, 0.35),
        ONE,
        &[(BODY, "limestone_shade", 1.2)],
    ),
    (
        "quoin",
        rounded(0.03, 0.1),
        ONE,
        &[(BODY, "limestone_shade", 1.25)],
    ),
    ("flagstone", BOX, ONE, &[(BODY, "limestone", 1.0)]),
    (
        "jamb_stone",
        rounded(0.03, 0.15),
        ONE,
        &[(BODY, "limestone", 1.0)],
    ),
    (
        "lintel",
        rounded(0.03, 0.0),
        ONE,
        &[(BODY, "limestone", 1.0)],
    ),
    (
        "chimney_cap",
        rounded(0.02, 0.0),
        ONE,
        &[(BODY, "limestone_shade", 1.0)],
    ),
    (
        "door_leaf",
        rounded(0.02, 0.25),
        ONE,
        &[(BODY, "livery", 1.0)],
    ),
    ("shutter", BOX, ONE, &[(BODY, "livery", 1.0)]),
    ("pane", BOX, ONE, &[(BODY, "pane", 1.0)]),
    (
        "ridge_cap",
        rounded(0.04, 0.0),
        ONE,
        &[(BODY, "roof", 0.85)],
    ),
    (
        "tiles",
        Shape::Tiles { taper: 1.0 },
        ONE,
        &[(BODY, "roof", 1.0)],
    ),
    (
        "tiles_hip",
        Shape::Tiles { taper: 0.3 },
        ONE,
        &[(BODY, "roof", 1.0)],
    ),
    (
        "tiles_hip_end",
        Shape::Tiles { taper: 0.0 },
        ONE,
        &[(BODY, "roof", 1.0)],
    ),
    ("post", rounded(0.02, 0.2), ONE, &[(BODY, "timber", 1.0)]),
    ("post_timber", BOX, ONE, &[(BODY, "timber", 1.0)]),
    ("rail", rounded(0.015, 0.0), ONE, &[(BODY, "timber", 1.0)]),
    (
        "brace",
        rounded(0.012, 0.0),
        ONE,
        &[(BODY, "timber_dark", 1.0)],
    ),
    ("beam_timber", BOX, ONE, &[(BODY, "timber_dark", 1.0)]),
    (
        "jetty",
        rounded(0.025, 0.0),
        ONE,
        &[(BODY, "timber_dark", 1.0)],
    ),
    ("frame_timber", BOX, ONE, &[(BODY, "timber_dark", 1.0)]),
    ("door", BOX, ONE, &[(BODY, "timber_dark", 1.0)]),
    ("stone_base", BOX, ONE, &[(BODY, "limestone", 1.0)]),
    (
        "stone_chimney",
        rounded(0.03, 0.1),
        ONE,
        &[(BODY, "limestone", 1.0)],
    ),
    ("sill_stone", BOX, ONE, &[(BODY, "limestone", 1.0)]),
    ("roof_tiles", BOX, ONE, &[(BODY, "terracotta", 1.0)]),
    ("roof_slate", BOX, ONE, &[(BODY, "slate_roof", 1.0)]),
    ("window", BOX, ONE, &[(BODY, "slate_roof", 1.0)]),
    ("flower_box", BOX, ONE, &[(BODY, "moss", 1.0)]),
    ("wall", rounded(0.04, 0.14), ONE, &[(BODY, "wall", 1.0)]),
    (
        "upper_wall",
        rounded(0.04, 0.08),
        ONE,
        &[(BODY, "upper_wall", 1.0)],
    ),
    (
        "worn_patch",
        rounded(0.01, 0.0),
        ONE,
        &[(BODY, "plaster_worn", 1.0)],
    ),
    ("gable", WEDGE, ONE, &[(BODY, "upper_wall", 0.97)]),
    (
        "gable_saltbox",
        Shape::Wedge { peak: -0.27 },
        ONE,
        &[(BODY, "upper_wall", 0.97)],
    ),
    ("crate", rounded(0.02, 0.3), ONE, &[(BODY, "timber", 1.2)]),
    ("knob", BUILT, Vec3::splat(0.07), &[(BODY, "brass", 1.0)]),
    (
        "shuttered_window",
        BUILT,
        Vec3::new(0.9, 0.63, 0.1),
        &[
            ("frame", "timber_dark", 1.0),
            ("shutter", "livery", 1.0),
            ("pane", "pane", 1.0),
        ],
    ),
    (
        "lantern",
        BUILT,
        Vec3::new(0.2, 0.5, 0.4),
        &[("bracket", "bed", 1.0), ("cage", "brass", 1.0)],
    ),
    (
        "mast",
        BUILT,
        Vec3::new(0.14, 2.0, 0.14),
        &[("pole", "timber_dark", 1.0), ("finial", "brass", 1.0)],
    ),
    (
        "barrel",
        BUILT,
        Vec3::new(0.46, 0.56, 0.46),
        &[(BODY, "timber", 1.0)],
    ),
    (
        "bush",
        BUILT,
        Vec3::new(2.0, 1.6, 2.0),
        &[("light", "foliage", 1.0), ("shade", "foliage_shade", 1.0)],
    ),
    (
        "woodpile",
        BUILT,
        Vec3::new(0.6, 0.5, 0.9),
        &[(BODY, "log", 1.0)],
    ),
    (
        "tower",
        BUILT,
        Vec3::new(1.84, 4.0, 1.84),
        &[
            ("stone", "limestone", 1.0),
            ("shade", "limestone_shade", 1.0),
        ],
    ),
    (
        "tower_roof",
        BUILT,
        Vec3::new(2.24, 1.95, 2.24),
        &[(BODY, "roof", 1.0), ("drift", "drift", 1.0)],
    ),
    ("lamp", MARK, Vec3::splat(0.1), &[]),
    ("banner", MARK, Vec3::splat(0.1), &[]),
];

impl Catalogue {
    /// The asset named `name`: the book's, else the built-in one.
    pub fn get(&self, name: &str) -> Option<Asset> {
        if let Some(asset) = self.0.get(name) {
            return Some(asset.clone());
        }
        let (_, shape, size, slots) =
            BUILTIN.iter().find(|(builtin, ..)| *builtin == name)?;
        Some(Asset {
            shape: shape.clone(),
            size: *size,
            slots: slots
                .iter()
                .map(|(slot, colour, shade)| {
                    let colour = (*colour).to_owned();
                    let paint = if *shade == 1.0 {
                        Paint::Palette(colour)
                    } else {
                        Paint::Shade(colour, *shade)
                    };
                    ((*slot).to_owned(), paint)
                })
                .collect(),
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
