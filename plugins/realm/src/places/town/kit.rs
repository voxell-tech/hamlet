//! The meshes every town shares: the yard's desk and engine, the
//! engine's wheel and its steam, the glass of a lantern, the common's
//! workbench and clockwork, and the trees of the gardens, each handed
//! to the engine once.

use glam::{Quat, Vec3};
use hamlet_engine_bevy::prelude::Handle;
use hamlet_engine_bevy::scene::{Mesh, MeshData};

use crate::build::mesh::Mb;
use crate::build::upload::{Draw, Unsent};
use crate::places::lantern::lantern_glass;
use crate::places::town::meter;
use crate::places::town::works::{self, COGS};
use crate::places::yard::{self, YardSettings};
use crate::world::flora;
use crate::world::palette::Palette;

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
    /// The common's workbench, the work laid on it and the glass of
    /// its lamp.
    Bench,
    Work,
    BenchLamp,
    /// A wheel of the bench's clockwork, the largest first.
    Cog(usize),
    /// A box one wide each way, for what is hit but not seen.
    Box,
    /// The common's budget meter: its stand, its light and its
    /// pointer.
    Meter,
    Level,
    Pace,
    /// A garden tree of a kind, from 0.
    Tree(usize),
}

impl Part {
    const ALL: usize = 12 + COGS + GARDEN_TREES;

    fn index(self) -> usize {
        match self {
            Self::Desk => 0,
            Self::Engine => 1,
            Self::Wheel => 2,
            Self::Ring => 3,
            Self::Lamp => 4,
            Self::Bench => 5,
            Self::Work => 6,
            Self::BenchLamp => 7,
            Self::Box => 8,
            Self::Meter => 9,
            Self::Level => 10,
            Self::Pace => 11,
            Self::Cog(k) => 12 + k.min(COGS - 1),
            Self::Tree(kind) => {
                12 + COGS + kind.min(GARDEN_TREES - 1)
            }
        }
    }

    fn of(index: usize) -> Self {
        match index {
            0 => Self::Desk,
            1 => Self::Engine,
            2 => Self::Wheel,
            3 => Self::Ring,
            4 => Self::Lamp,
            5 => Self::Bench,
            6 => Self::Work,
            7 => Self::BenchLamp,
            8 => Self::Box,
            9 => Self::Meter,
            10 => Self::Level,
            11 => Self::Pace,
            k if k < 12 + COGS => Self::Cog(k - 12),
            kind => Self::Tree(kind - 12 - COGS),
        }
    }

    fn build(self, pal: &Palette, yard: &YardSettings) -> Mb {
        match self {
            Self::Desk => yard::desk(pal, yard),
            Self::Engine => yard::engine(pal, yard),
            Self::Wheel => yard::wheel(pal),
            Self::Ring => yard::ring(),
            Self::Lamp => lantern_glass(0.075, 0.17),
            Self::Bench => works::bench(pal),
            Self::Work => works::work(pal),
            Self::BenchLamp => lantern_glass(0.09, 0.22),
            Self::Cog(k) => works::cog(pal, k),
            Self::Box => {
                let mut mb = Mb::new();
                mb.rbox(
                    Vec3::ZERO,
                    Vec3::splat(0.5),
                    0.0,
                    Quat::IDENTITY,
                    Vec3::ONE,
                    0.0,
                );
                mb
            }
            Self::Meter => meter::stand(pal),
            Self::Level => meter::level(),
            Self::Pace => meter::pace(pal),
            Self::Tree(kind) => flora::mesh(pal, kind),
        }
    }
}

/// The shared meshes, built and handed over a few at a time. A part
/// the engine refused has none.
pub struct Kit {
    meshes: Vec<Option<Handle<Mesh>>>,
    /// Parts built so far, and the last one built while it waits for
    /// room to be handed over.
    made: usize,
    waiting: Option<MeshData>,
}

impl Kit {
    pub fn new() -> Self {
        Self {
            meshes: (0..Part::ALL).map(|_| None).collect(),
            made: 0,
            waiting: None,
        }
    }

    /// Builds and hands over what is missing, while the frame has
    /// time. True once every part is done.
    pub fn fill(
        &mut self,
        pal: &Palette,
        yard: &YardSettings,
        draw: &mut Draw,
    ) -> bool {
        while self.made < Part::ALL {
            if !draw.left() {
                return false;
            }
            let data = self.waiting.take().unwrap_or_else(|| {
                Part::of(self.made).build(pal, yard).data()
            });
            match draw.mesh(data) {
                Ok(mesh) => self.meshes[self.made] = Some(mesh),
                Err(Unsent::Full(data)) => {
                    self.waiting = Some(data);
                    return false;
                }
                Err(Unsent::Refused(why)) => {
                    hamlet_engine_bevy::warn(&format!(
                        "a town's kit: {why}"
                    ));
                }
            }
            self.made += 1;
        }
        true
    }

    /// The mesh of `part`, once it is there.
    pub fn get(&self, part: Part) -> Option<&Handle<Mesh>> {
        self.meshes[part.index()].as_ref()
    }
}
