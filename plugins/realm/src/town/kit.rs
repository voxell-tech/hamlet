//! The meshes every town shares: the yard's desk and engine, the
//! engine's wheel and its steam, the glass of a lantern and the trees
//! of the gardens, each handed to the engine once.

use hamlet_engine_sdk::scene::Mesh;

use crate::flora;
use crate::lantern::lantern_glass;
use crate::mesh::Mb;
use crate::palette::Palette;
use crate::upload::{Unsent, Upload};
use crate::yard::{self, YardSettings};

/// Kinds of tree a garden may have: the broadleaves.
pub const GARDEN_TREES: usize = 5;

/// The shared meshes, by their place in [`Kit::meshes`].
#[derive(Clone, Copy)]
pub enum Part {
    Desk,
    Engine,
    Wheel,
    Ring,
    Lamp,
    /// A garden tree of a kind, from 0.
    Tree(usize),
}

impl Part {
    const ALL: usize = 5 + GARDEN_TREES;

    fn index(self) -> usize {
        match self {
            Self::Desk => 0,
            Self::Engine => 1,
            Self::Wheel => 2,
            Self::Ring => 3,
            Self::Lamp => 4,
            Self::Tree(kind) => 5 + kind.min(GARDEN_TREES - 1),
        }
    }

    fn of(index: usize) -> Self {
        match index {
            0 => Self::Desk,
            1 => Self::Engine,
            2 => Self::Wheel,
            3 => Self::Ring,
            4 => Self::Lamp,
            kind => Self::Tree(kind - 5),
        }
    }

    fn build(self, pal: &Palette, yard: &YardSettings) -> Mb {
        match self {
            Self::Desk => yard::desk(pal, yard),
            Self::Engine => yard::engine(pal, yard),
            Self::Wheel => yard::wheel(pal),
            Self::Ring => yard::ring(),
            Self::Lamp => lantern_glass(0.075, 0.17),
            Self::Tree(kind) => flora::mesh(pal, kind),
        }
    }
}

/// The shared meshes, handed over a few at a time.
pub struct Kit {
    meshes: Vec<Option<Mesh>>,
}

impl Kit {
    pub fn new() -> Self {
        Self {
            meshes: (0..Part::ALL).map(|_| None).collect(),
        }
    }

    /// Hands over what is missing, within `upload`. True once every
    /// mesh is there.
    pub fn fill(
        &mut self,
        pal: &Palette,
        yard: &YardSettings,
        upload: &mut Upload,
    ) -> bool {
        for (index, slot) in self.meshes.iter_mut().enumerate() {
            if slot.is_some() {
                continue;
            }
            if !upload.left() {
                return false;
            }
            match Part::of(index).build(pal, yard).upload(upload) {
                Ok(mesh) => *slot = Some(mesh),
                Err(Unsent::Full(_)) => return false,
                Err(Unsent::Refused(why)) => {
                    hamlet_engine_sdk::warn(&format!(
                        "a town's kit: {why}"
                    ));
                    return false;
                }
            }
        }
        true
    }

    /// The mesh of `part`, once it is there.
    pub fn get(&self, part: Part) -> Option<&Mesh> {
        self.meshes[part.index()].as_ref()
    }
}
