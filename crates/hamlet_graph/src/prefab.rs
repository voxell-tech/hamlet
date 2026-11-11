//! Prefabs: meshes of the library, made in Blender, put together and
//! changed in Hamlet. Each mesh put in is an object at the top of the
//! prefab, named by its [`Placed`] name; the objects of its `.glb`
//! scene are under it, each named by the path of node names from
//! there, as Blender's outliner shows them: `main_roof/Roof/Tiles`. A
//! name used twice under one parent takes its index, `Window#2`. Each
//! object keeps what the file gives it unless its part says
//! otherwise: a part may give it a shader graph or a material of the
//! library in place of the file's own, and move, turn and scale it on
//! top of the file's transform. A part whose object the file no
//! longer has is kept, so a renamed object can be given its settings
//! back by hand.

use std::collections::BTreeMap;

use bevy_math::{Quat, Vec3};
use serde::{Deserialize, Serialize};

/// The meshes of the library put together, and the parts of them
/// changed, by their objects' paths.
#[derive(
    Clone, PartialEq, Debug, Default, Serialize, Deserialize,
)]
#[serde(from = "Written")]
pub struct Prefab {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub meshes: Vec<Placed>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub parts: BTreeMap<String, Part>,
}

/// A mesh of the library put in a prefab: its object's name there,
/// the first part of the paths of everything under it, and the mesh.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Placed {
    pub name: String,
    pub mesh: String,
}

/// A prefab as its file may hold it: as now, or as a prefab of one
/// mesh, `model`, was written, its parts' paths starting in the
/// mesh's scene.
#[derive(Deserialize)]
struct Written {
    #[serde(default)]
    model: String,
    #[serde(default)]
    meshes: Vec<Placed>,
    #[serde(default)]
    parts: BTreeMap<String, Part>,
}

impl From<Written> for Prefab {
    fn from(written: Written) -> Self {
        let Written {
            model,
            meshes,
            parts,
        } = written;
        if model.is_empty() || !meshes.is_empty() {
            return Self { meshes, parts };
        }
        Self {
            parts: parts
                .into_iter()
                .map(|(path, part)| (format!("{model}/{path}"), part))
                .collect(),
            meshes: vec![Placed {
                name: model.clone(),
                mesh: model,
            }],
        }
    }
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
    /// The name shown for the object in place of the file's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
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
            name: None,
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
    /// A prefab of the mesh `mesh` alone that changes nothing, or of
    /// none when it is empty.
    pub fn of(mesh: &str) -> Self {
        let mut prefab = Self::default();
        if !mesh.is_empty() {
            prefab.add(mesh);
        }
        prefab
    }

    /// Puts the mesh `mesh` in, named for it, or with the nearest
    /// index after its name when the name is taken, and gives the
    /// name.
    pub fn add(&mut self, mesh: &str) -> String {
        let taken = |name: &str| {
            self.meshes.iter().any(|placed| placed.name == name)
        };
        let name = if taken(mesh) {
            (2..)
                .map(|index| format!("{mesh}#{index}"))
                .find(|name| !taken(name))
                .unwrap_or_default()
        } else {
            mesh.to_owned()
        };
        self.meshes.push(Placed {
            name: name.clone(),
            mesh: mesh.to_owned(),
        });
        name
    }

    /// Takes out the mesh put in as `name`, with the parts of its
    /// objects; whether it was in.
    pub fn remove(&mut self, name: &str) -> bool {
        let before = self.meshes.len();
        self.meshes.retain(|placed| placed.name != name);
        let under = format!("{name}/");
        self.parts.retain(|path, _| {
            path != name && !path.starts_with(&under)
        });
        self.meshes.len() != before
    }

    /// Whether the mesh `mesh` is put in.
    pub fn uses(&self, mesh: &str) -> bool {
        self.meshes.iter().any(|placed| placed.mesh == mesh)
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
    fn a_prefab_of_one_mesh_as_it_was_written_reads_as_one_put_in() {
        let text = r#"(
            model: "hand_tower",
            parts: { "Door": (offset: (0.0, 0.0, 0.05)) },
        )"#;
        let prefab = Prefab::from_ron(text).unwrap();
        assert_eq!(
            prefab.meshes,
            [Placed {
                name: "hand_tower".into(),
                mesh: "hand_tower".into(),
            }]
        );
        assert!(prefab.parts.contains_key("hand_tower/Door"));
        // And it is written back as it is now.
        let again =
            Prefab::from_ron(&prefab.to_ron().unwrap()).unwrap();
        assert_eq!(again, prefab);
    }

    #[test]
    fn a_mesh_put_in_twice_takes_an_index_and_goes_with_its_parts() {
        let mut prefab = Prefab::of("window");
        assert_eq!(prefab.add("window"), "window#2");
        prefab.set(
            "window#2/Frame",
            Part {
                scale: Vec3::splat(2.0),
                ..Part::default()
            },
        );
        assert!(prefab.remove("window#2"));
        assert!(prefab.parts.is_empty());
        assert_eq!(prefab.meshes.len(), 1);
        assert!(prefab.uses("window"));
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
