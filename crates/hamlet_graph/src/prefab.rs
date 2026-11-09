//! Prefabs: a mesh of the library, made in Blender, with some of its
//! objects changed in Hamlet. An object is a node of the mesh's
//! `.glb` scene, named by the path of node names from the scene's
//! root, as Blender's outliner shows it; a name used twice under one
//! parent takes its index, `Window#2`. Each object keeps what the
//! file gives it unless its part says otherwise: a part may give it a
//! shader graph or a material of the library in place of the file's
//! own, and move, turn and scale it on top of the file's transform. A
//! part whose object the file no longer has is kept, so a renamed
//! object can be given its settings back by hand.

use std::collections::BTreeMap;

use bevy_math::{Quat, Vec3};
use serde::{Deserialize, Serialize};

/// A mesh of the library by name and the parts of it changed, by
/// their objects' paths.
#[derive(
    Clone, PartialEq, Debug, Default, Serialize, Deserialize,
)]
pub struct Prefab {
    pub model: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub parts: BTreeMap<String, Part>,
}

/// What an object is drawn with.
#[derive(
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Debug,
    Default,
    Serialize,
    Deserialize,
)]
pub enum Finish {
    /// The material the file gives it.
    #[default]
    Own,
    /// A shader graph of the library, by name, at its inputs'
    /// defaults.
    Shader(String),
    /// A material of the library, by name.
    Material(String),
}

impl Finish {
    pub fn is_own(&self) -> bool {
        *self == Finish::Own
    }
}

/// How one object differs from what the file gives it.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Part {
    #[serde(skip_serializing_if = "Finish::is_own")]
    pub finish: Finish,
    /// Added to the object's place in the file, in its parent's
    /// space.
    #[serde(skip_serializing_if = "is_zero")]
    pub offset: Vec3,
    /// Turns the object on top of the file's turn.
    #[serde(skip_serializing_if = "is_still")]
    pub turn: Quat,
    /// Scales the object on top of the file's scale.
    #[serde(skip_serializing_if = "is_one")]
    pub scale: Vec3,
}

impl Default for Part {
    fn default() -> Self {
        Self {
            finish: Finish::Own,
            offset: Vec3::ZERO,
            turn: Quat::IDENTITY,
            scale: Vec3::ONE,
        }
    }
}

impl Part {
    /// Whether it changes nothing.
    pub fn is_plain(&self) -> bool {
        *self == Self::default()
    }
}

fn is_zero(offset: &Vec3) -> bool {
    *offset == Vec3::ZERO
}

fn is_still(turn: &Quat) -> bool {
    *turn == Quat::IDENTITY
}

fn is_one(scale: &Vec3) -> bool {
    *scale == Vec3::ONE
}

impl Prefab {
    /// A prefab of the mesh `model` that changes nothing.
    pub fn of(model: &str) -> Self {
        Self {
            model: model.to_owned(),
            parts: BTreeMap::new(),
        }
    }

    /// How the object at `path` is changed.
    pub fn part(&self, path: &str) -> Part {
        self.parts.get(path).cloned().unwrap_or_default()
    }

    /// Changes the object at `path` as `part` says, keeping no part
    /// for an object it changes nothing of.
    pub fn set(&mut self, path: &str, part: Part) {
        if part.is_plain() {
            self.parts.remove(path);
        } else {
            self.parts.insert(path.to_owned(), part);
        }
    }

    /// The paths of the parts whose objects are not among `objects`,
    /// the paths the file has now.
    pub fn missing<'a>(
        &'a self,
        objects: &[String],
    ) -> impl Iterator<Item = &'a str> {
        self.parts
            .keys()
            .filter(|path| !objects.contains(path))
            .map(String::as_str)
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
    fn a_part_that_changes_nothing_is_not_kept() {
        let mut prefab = Prefab::of("hand_tower");
        prefab.set(
            "Tower/Roof",
            Part {
                finish: Finish::Shader("slate".into()),
                ..Part::default()
            },
        );
        prefab.set("Door", Part::default());
        assert_eq!(prefab.parts.len(), 1);
        prefab.set("Tower/Roof", Part::default());
        assert!(prefab.parts.is_empty());
    }

    #[test]
    fn a_prefab_reads_back_as_it_was_written() {
        let mut prefab = Prefab::of("hand_tower");
        prefab.set(
            "Door",
            Part {
                offset: Vec3::new(0.0, 0.0, 0.05),
                turn: Quat::from_rotation_z(0.5),
                ..Part::default()
            },
        );
        prefab.set(
            "Tower/Wall",
            Part {
                finish: Finish::Material("plaster".into()),
                ..Part::default()
            },
        );
        let text = prefab.to_ron().unwrap();
        assert_eq!(Prefab::from_ron(&text).unwrap(), prefab);
        // Only what a part changes is written.
        assert!(!text.contains("scale"));
    }

    #[test]
    fn a_part_whose_object_is_gone_is_missing() {
        let mut prefab = Prefab::of("hand_tower");
        prefab.set(
            "Old/Door",
            Part {
                scale: Vec3::splat(2.0),
                ..Part::default()
            },
        );
        let objects = vec!["Door".to_owned()];
        assert_eq!(
            prefab.missing(&objects).collect::<Vec<_>>(),
            ["Old/Door"]
        );
    }
}
