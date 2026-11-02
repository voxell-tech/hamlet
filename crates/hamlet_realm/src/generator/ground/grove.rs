//! The trees and plants of one tile.

use bevy::prelude::*;

use super::{DetailSettings, Tile};
use crate::generator::flora::{self, Tree, plants};
use crate::generator::land::Land;

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
}

/// The trees of a tile as they come out of a background task.
#[derive(Default)]
pub struct Grove {
    /// Those that stand as themselves.
    pub trees: Vec<Tree>,
    /// One mesh for the tile: the leaves fallen about the trees that
    /// stand as themselves, or all of them as one, where the tile is
    /// far enough for that. None where there is nothing to draw.
    pub mesh: Option<Mesh>,
    /// Whether the mesh is of plants, which sway, and not of trees
    /// far off, which keep still.
    pub sways: bool,
}

pub fn grove(land: &Land, tile: Tile, stand: Stand) -> Grove {
    let chunk = land.set.land.chunk;
    let (lo, side) = (tile.corner(chunk), tile.side(chunk));
    match stand {
        Stand::Trees => {
            let (hi, sparse) = (lo + side, tile.level > 0);
            let trees = flora::trees(land, lo, hi, 1);
            Grove {
                mesh: plants::plants(land, lo, hi, &trees, sparse),
                trees,
                sways: true,
            }
        }
        Stand::Canopy { every, crowns } => {
            let trees = flora::trees(land, lo, lo + side, every);
            Grove {
                mesh: (!trees.is_empty()).then(|| {
                    flora::canopy(&land.pal, &trees, crowns)
                }),
                trees: Vec::new(),
                sways: false,
            }
        }
    }
}
