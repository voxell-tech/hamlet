//! The woods of tiles built before, kept so that a tile the view
//! comes back to is drawn from what was built for it, not built and
//! meshed anew.

use std::collections::VecDeque;

use bevy::platform::collections::HashMap;
use bevy::prelude::*;

use crate::generator::flora::Tree;
use crate::generator::flora::plants::Pad;
use crate::generator::ground::{Grove, Tile};

/// Bytes of mesh data kept at most. The tiles used longest ago go
/// first.
const MOST: usize = 64 << 20;

/// A tile's woods as they come to be drawn: built just now, or taken
/// from the shelf.
pub(super) enum Grown {
    New(Box<Grove>),
    Kept(Kept),
}

/// A tile's woods as built: the trees that stand as themselves, and
/// the tile's one mesh already among the assets, with its bytes, and
/// whether that mesh sways, and the lily pads.
#[derive(Clone)]
pub(super) struct Kept {
    pub(super) trees: Vec<Tree>,
    pub(super) mesh: Option<(Handle<Mesh>, usize)>,
    pub(super) sways: bool,
    pub(super) pads: Vec<Pad>,
}

impl Kept {
    fn bytes(&self) -> usize {
        self.mesh.as_ref().map_or(0, |(_, bytes)| *bytes)
    }
}

/// What was built for each tile lately, for the land of `seed`.
#[derive(Default)]
pub(super) struct Shelf {
    seed: u32,
    kept: HashMap<Tile, Kept>,
    /// The tiles kept, the one used longest ago first.
    order: VecDeque<Tile>,
    bytes: usize,
}

impl Shelf {
    /// What was built for `tile` of the land of `seed`, if it is
    /// kept.
    pub(super) fn get(
        &mut self,
        seed: u32,
        tile: Tile,
    ) -> Option<Kept> {
        if seed != self.seed {
            return None;
        }
        let kept = self.kept.get(&tile)?.clone();
        self.order.retain(|t| *t != tile);
        self.order.push_back(tile);
        Some(kept)
    }

    /// Keeps what was built for `tile` of the land of `seed`, letting
    /// go of those used longest ago while there is too much. A land
    /// of another seed has none of the others.
    pub(super) fn put(&mut self, seed: u32, tile: Tile, kept: Kept) {
        if seed != self.seed {
            *self = Self { seed, ..default() };
        }
        self.forget(tile);
        self.bytes += kept.bytes();
        self.kept.insert(tile, kept);
        self.order.push_back(tile);
        while self.bytes > MOST
            && let Some(old) = self.order.front().copied()
        {
            self.forget(old);
        }
    }

    /// Lets go of the tiles within `hold` of `at`, whose land
    /// changed.
    pub(super) fn redo(&mut self, chunk: f32, at: Vec2, hold: f32) {
        let near = self
            .kept
            .keys()
            .filter(|tile| {
                let reach = tile.side(chunk) * 0.71 + hold;
                tile.middle(chunk).distance(at) < reach
            })
            .copied()
            .collect::<Vec<_>>();
        for tile in near {
            self.forget(tile);
        }
    }

    /// Lets go of every tile.
    pub(super) fn clear(&mut self) {
        *self = Self {
            seed: self.seed,
            ..default()
        };
    }

    fn forget(&mut self, tile: Tile) {
        if let Some(kept) = self.kept.remove(&tile) {
            self.bytes -= kept.bytes();
            self.order.retain(|t| *t != tile);
        }
    }
}
