//! The ground as it is drawn, for a mark of light to be laid on.

use bevy::prelude::*;
use hamlet_vfx::mark::{Lay, heights};

use super::Robot;
use super::lot::footing;
use crate::generator::land::Land;

/// The ground as it is drawn: the land, the paving of a common and
/// the paving of every yard. A mark of light is laid on it
/// (`hamlet_vfx::mark`).
#[derive(bevy::ecs::system::SystemParam)]
pub struct Floor<'w, 's> {
    land: Res<'w, Land>,
    robots: Query<'w, 's, &'static Robot>,
    images: ResMut<'w, Assets<Image>>,
}

impl Floor<'_, '_> {
    /// Farthest over or under the ground a mark still falls: over a
    /// path and the flags of a yard, and no higher.
    const NEAR: f32 = 0.16;

    /// Points along the side of the heights a mark is laid with.
    const SIDE: usize = 48;

    /// The height of the ground at `p`.
    pub fn at(&self, p: Vec2) -> f32 {
        let yard = self
            .robots
            .iter()
            .filter_map(|robot| robot.lot.paved(p))
            .fold(f32::MIN, f32::max);
        footing(&self.land, p).max(yard)
    }

    /// The ground about `centre`, as far as `half` to every side,
    /// for a mark to be laid on. Its heights are a picture, which is
    /// the caller's to remove with the mark.
    pub fn under(&mut self, centre: Vec3, half: f32) -> Lay {
        let middle = centre.xz();
        // Only the yards within reach are asked.
        let near = self
            .robots
            .iter()
            .map(|robot| robot.lot)
            .filter(|lot| {
                lot.origin.xz().distance(middle) < half * 1.5 + 12.0
            })
            .collect::<Vec<_>>();
        let land = &self.land;
        let picture = heights(Self::SIDE, |from| {
            let p = middle + from * half;
            let yard = near
                .iter()
                .filter_map(|lot| lot.paved(p))
                .fold(f32::MIN, f32::max);
            footing(land, p).max(yard) - centre.y
        });
        Lay {
            centre: middle,
            half,
            base: centre.y,
            heights: self.images.add(picture),
            near: Self::NEAR,
        }
    }

    /// Lets go of the heights a mark lay on.
    pub fn forget(&mut self, heights: &Handle<Image>) {
        self.images.remove(heights);
    }
}
