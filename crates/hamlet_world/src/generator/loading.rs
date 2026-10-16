//! The ground, the trees, houses, roads and commons as they come
//! out of the background tasks that build them.

mod builds;
mod lanes;
mod layer;
mod stream;
#[cfg(test)]
mod tests;

use bevy::prelude::*;

pub(super) use self::builds::{finish_loading, fit_out};
pub(super) use self::lanes::{Lanes, clear_lanes};
use self::layer::Layer;
pub(super) use self::stream::stream;
use super::building::Builds;
use super::flora;
use super::ground::{Grove, Sight, Sod, Tile};

/// Everything being built in the background, and what of the
/// ground is in the world.
#[derive(Resource, Default)]
pub struct Loading {
    ground: Layer<Sod>,
    groves: Layer<Grove>,
    /// The views the tiles were last chosen for: the camera's, and
    /// the one it is being taken to.
    sight: Option<(Sight, Sight)>,
    /// The place of each site at that time, and the ground then
    /// held.
    sites: Vec<Vec2>,
    holds: Vec<(Vec2, f32)>,
    /// Trees found and not yet planted, each with its tile's
    /// entity.
    waiting: Vec<(Entity, flora::Tree)>,
    /// The ground about the first view is in the world.
    landed: bool,
    pub(super) builds: Builds,
}

impl Loading {
    /// True when everything asked for so far is in the world.
    pub fn done(&self) -> bool {
        self.settled() && self.builds.is_empty()
    }

    /// True when the ground and the trees of the view are in the
    /// world.
    fn settled(&self) -> bool {
        self.waiting.is_empty()
            && self.ground.settled
            && self.groves.settled
    }

    /// Tiles of ground and of trees drawn, and tiles being built.
    #[cfg(test)]
    pub fn tiles(&self) -> (usize, usize) {
        (
            self.ground.drawn().count() + self.groves.drawn().count(),
            self.ground.building + self.groves.building,
        )
    }

    /// Bytes of mesh data in the ground that is drawn, and in the
    /// far trees.
    pub fn bytes(&self) -> (usize, usize) {
        (
            self.ground.drawn().map(|p| p.bytes).sum(),
            self.groves.drawn().map(|p| p.bytes).sum(),
        )
    }
}

/// A tile of ground that is drawn.
#[derive(Component)]
pub struct Ground(pub Tile);

/// The trees of a tile.
#[derive(Component)]
pub(super) struct Wood;

/// A tree that stands as itself.
#[derive(Component)]
pub(super) struct Standing;

/// The plain ground that shows where no tile is drawn yet.
#[derive(Component)]
pub(super) struct Bed;
