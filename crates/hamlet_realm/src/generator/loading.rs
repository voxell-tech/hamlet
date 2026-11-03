//! The trees, houses, roads and commons as they come out of the
//! background tasks that build them, and the ground and the sea in
//! patches about the eye.

mod builds;
mod lanes;
mod layer;
mod shelf;
mod stream;
#[cfg(test)]
mod tests;

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

pub(super) use self::builds::finish_loading;
pub(super) use self::lanes::{Lanes, clear_lanes};
use self::layer::Layer;
use self::shelf::{Grown, Shelf};
pub(super) use self::stream::stream;
use super::building::Builds;
use super::flora;
use super::ground::Sight;
use crate::sky::Quality;

/// Everything being built in the background, and what of the
/// trees is in the world.
#[derive(Resource, Default)]
pub struct Loading {
    groves: Layer<Grown>,
    /// The woods built for tiles lately, drawn from again when the
    /// view comes back to them.
    shelf: Shelf,
    /// The views the tiles were last chosen for: the camera's, and
    /// the one it is being taken to.
    sight: Option<(Sight, Sight)>,
    /// The place of each site at that time, and the ground then
    /// held.
    sites: Vec<Vec2>,
    holds: Vec<(Vec2, f32)>,
    /// The points the island then reached about.
    shores: Vec<Vec2>,
    /// Trees found and not yet planted, each with its tile's
    /// entity.
    waiting: Vec<(Entity, flora::Tree)>,
    /// The ground about the first view is in the world.
    landed: bool,
    pub(super) builds: Builds,
}

impl Loading {
    /// True when everything asked for so far is in the world.
    pub fn is_done(&self) -> bool {
        self.is_settled() && self.builds.is_empty()
    }

    /// True when the trees of the view are in the world.
    fn is_settled(&self) -> bool {
        self.waiting.is_empty() && self.groves.settled
    }

    /// Tiles of trees drawn, and tiles being built.
    #[cfg(test)]
    pub fn tiles(&self) -> (usize, usize) {
        (self.groves.drawn().count(), self.groves.building)
    }

    /// Each tile of trees drawn, with the entity it is drawn as.
    #[cfg(test)]
    fn woods(&self) -> Vec<(super::ground::Tile, Entity)> {
        self.groves.drawn_tiles()
    }

    /// Bytes of mesh data in the far trees.
    pub fn bytes(&self) -> usize {
        self.groves.drawn().map(|p| p.bytes).sum()
    }
}

/// A patch of the ground, as `hamlet_vfx::patch` lays them.
#[derive(Component)]
pub struct Terrain;

/// The trees of a tile.
#[derive(Component)]
pub(super) struct Wood;

/// A tree that stands as itself.
#[derive(Component)]
pub(super) struct Standing;

/// The plain ground that shows beyond the ground's patches.
#[derive(Component)]
pub(super) struct Bed;

/// A patch of the water over everything that lies under its height.
#[derive(Component)]
pub(super) struct Sea;

/// Keeps [`NotShadowCaster`] on the trees when the [`Quality`] draws
/// no tree shadows, and off them when it does.
pub(super) fn shade_trees(
    mut commands: Commands,
    quality: Res<Quality>,
    casting: Query<
        Entity,
        (Or<(With<Wood>, With<Standing>)>, Without<NotShadowCaster>),
    >,
    shadowless: Query<
        Entity,
        (Or<(With<Wood>, With<Standing>)>, With<NotShadowCaster>),
    >,
) {
    if quality.has_tree_shadows() {
        for tree in &shadowless {
            commands.entity(tree).remove::<NotShadowCaster>();
        }
    } else {
        for tree in &casting {
            commands.entity(tree).try_insert(NotShadowCaster);
        }
    }
}
