//! The trees of one tile.

use bevy::prelude::*;

use super::{DetailSettings, Tile};
use crate::generator::flora::{self, Tree};
use crate::generator::land::Land;

/// The drawing of a tile's trees.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Stand {
    /// Each as itself.
    Trees,
    /// One mesh for one tree in `every`, as crowns or as specks.
    Canopy { every: u64, crowns: bool },
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
}

/// The trees of a tile as they come out of a background task.
#[derive(Default)]
pub struct Grove {
    /// Those that stand as themselves.
    pub trees: Vec<Tree>,
    /// All of them as one mesh, where the tile is far enough for
    /// that and has any.
    pub canopy: Option<Mesh>,
}

pub fn grove(land: &Land, tile: Tile, stand: Stand) -> Grove {
    let chunk = land.set.land.chunk;
    let (lo, side) = (tile.corner(chunk), tile.side(chunk));
    match stand {
        Stand::Trees => Grove {
            trees: flora::trees(land, lo, lo + side, 1),
            canopy: None,
        },
        Stand::Canopy { every, crowns } => {
            let trees = flora::trees(land, lo, lo + side, every);
            Grove {
                canopy: (!trees.is_empty()).then(|| {
                    flora::canopy(&land.pal, &trees, crowns)
                }),
                trees: Vec::new(),
            }
        }
    }
}
