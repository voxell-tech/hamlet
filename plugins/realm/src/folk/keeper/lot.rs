//! The ground a house stands on, as its robot walks it.

use glam::{Quat, Vec2, Vec3, Vec3Swizzles};

use crate::places::yard::{Station, Yard};
use crate::world::land::Land;
use crate::world::layout::Plot;

/// The ground a house stands on, for placing its robot.
#[derive(Clone, Copy, Debug)]
pub(super) struct Lot {
    /// The house's middle.
    pub(super) origin: Vec3,
    pub(super) yaw: f32,
    pub(super) yard: Yard,
    /// The spot the robot stands at with nothing to do.
    pub(super) porch: Vec3,
}

/// The height a robot stands at: on the common's paving or on the
/// ground.
pub(super) fn footing(land: &Land, p: Vec2) -> f32 {
    let town = &land.set.town;
    if land.site_dist(p) < town.plaza_radius - 0.1 {
        land.set.land.site_y + town.plaza_lift
    } else {
        land.surface(p)
    }
}

impl Lot {
    pub(super) fn new(land: &Land, spot: &Plot, yard: Yard) -> Self {
        let porch = spot.porch();
        Self {
            origin: land.on_ground(spot.at),
            yaw: spot.yaw,
            yard,
            porch: Vec3::new(porch.x, footing(land, porch), porch.y),
        }
    }

    pub(super) fn local(&self, p: Vec3) -> Vec3 {
        Quat::from_rotation_y(-self.yaw) * (p - self.origin)
    }

    /// A point of the house's frame, on the yard's paving or on the
    /// ground.
    pub(super) fn world(&self, land: &Land, p: Vec3) -> Vec3 {
        let at = self.origin + Quat::from_rotation_y(self.yaw) * p;
        let y = if self.yard.holds(p) {
            self.origin.y + self.yard.shape.terrace
        } else {
            footing(land, at.xz())
        };
        at.with_y(y)
    }

    /// The spot of a robot at `station`, and the way it faces. None
    /// for a station that is wherever the robot is.
    pub(super) fn stand(
        &self,
        land: &Land,
        station: Station,
        beat: f32,
    ) -> Option<(Vec3, f32)> {
        match station {
            Station::Here => None,
            Station::Porch => Some((self.porch, self.yaw)),
            _ => {
                let (p, yaw) = self.yard.stand(station, beat);
                Some((self.world(land, p), self.yaw + yaw))
            }
        }
    }

    /// True when `p` is away from the house, too far to walk to it
    /// about the yard.
    pub(super) fn away(&self, land: &Land, p: Vec3) -> bool {
        let town = &land.set.town;
        p.xz().distance(self.origin.xz())
            > town.house_radius + town.robot_clearance + 0.5
    }

    pub(super) fn way(
        &self,
        land: &Land,
        from: Vec3,
        to: Vec3,
    ) -> Vec<Vec3> {
        let mut way = self
            .yard
            .way(self.local(from), self.local(to))
            .into_iter()
            .map(|p| self.world(land, p))
            .collect::<Vec<_>>();
        if let Some(first) = way.first_mut() {
            *first = from;
        }
        way
    }
}
