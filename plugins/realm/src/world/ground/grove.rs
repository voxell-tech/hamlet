//! The trees of one tile, found a column of the trees' grid at a
//! time.

use std::ops::RangeInclusive;

use glam::Vec2;
use hamlet_engine_bevy::scene::MeshData;

use crate::world::flora::{self, Tree};
use crate::world::ground::{DetailSettings, Tile};
use crate::world::land::Land;

/// The drawing of a tile's trees.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Stand {
    /// Each as itself.
    Trees,
    /// One mesh for one tree in `every`, as crowns or as specks.
    Canopy { every: u32, crowns: bool },
}

impl Stand {
    pub fn of(detail: &DetailSettings, tile: Tile) -> Self {
        let level = tile.level;
        if level < detail.tree_levels {
            return Stand::Trees;
        }
        // Past the crowns, each size up keeps fewer: a half, then
        // an eighth, then half of that again.
        let past = (level + 1).saturating_sub(detail.crown_levels);
        Stand::Canopy {
            every: match past {
                0 => 1,
                1 => 2,
                more => 4 << (more - 1).min(5),
            },
            crowns: past == 0,
        }
    }

    /// One tree in this many is kept.
    fn every(self) -> u32 {
        match self {
            Stand::Trees => 1,
            Stand::Canopy { every, .. } => every,
        }
    }
}

/// The trees of a tile, found.
#[derive(Default)]
pub struct Grove {
    /// Those that stand as themselves.
    pub trees: Vec<Tree>,
    /// All of them as one mesh, where the tile is far enough for
    /// that and has any.
    pub canopy: Option<MeshData>,
}

/// The trees of a tile being found.
pub struct GroveJob {
    stand: Stand,
    lo: Vec2,
    hi: Vec2,
    /// The columns of the trees' grid left to look over.
    columns: RangeInclusive<i32>,
    trees: Vec<Tree>,
    /// The grove found, while it waits for room to be handed over.
    ready: Option<Grove>,
}

impl GroveJob {
    pub fn new(
        land: &Land,
        detail: &DetailSettings,
        tile: Tile,
    ) -> Self {
        let chunk = land.set.land.chunk;
        let (lo, side) = (tile.corner(chunk), tile.side(chunk));
        Self {
            stand: Stand::of(detail, tile),
            lo,
            hi: lo + side,
            columns: flora::columns(land, lo, lo + side),
            trees: Vec::new(),
            ready: None,
        }
    }

    /// Keeps the grove found until there is room to hand it over.
    pub fn wait(&mut self, grove: Grove) {
        self.ready = Some(grove);
    }

    /// One column more of the tile's trees; the grove once they are
    /// all found.
    pub fn step(&mut self, land: &Land) -> Option<Grove> {
        if let Some(grove) = self.ready.take() {
            return Some(grove);
        }
        if let Some(column) = self.columns.next() {
            flora::column_trees(
                land,
                self.lo,
                self.hi,
                self.stand.every(),
                column,
                &mut self.trees,
            );
            return None;
        }
        let trees = std::mem::take(&mut self.trees);
        Some(match self.stand {
            Stand::Trees => Grove {
                trees,
                canopy: None,
            },
            Stand::Canopy { crowns, .. } => Grove {
                canopy: (!trees.is_empty()).then(|| {
                    flora::canopy(&land.pal, &trees, crowns).data()
                }),
                trees: Vec::new(),
            },
        })
    }
}
