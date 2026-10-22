//! The ground a house stands on, as its robot walks it, and the floor
//! of the land and every yard that marks of light are laid on.

use glam::{Quat, Vec2, Vec3, Vec3Swizzles};

use crate::places::yard::{Station, Yard};
use crate::world::land::{Land, Surfaces};
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

/// The ground as it is drawn: the land, the paving of a common and
/// the paving of every yard.
pub struct Floor<'a> {
    land: &'a Land,
    lots: Vec<Lot>,
}

/// The floor about one place, with the yards within its reach.
pub struct NearFloor<'a> {
    land: &'a Land,
    lots: Vec<Lot>,
    surfaces: Surfaces<'a>,
}

/// The height a robot stands at: on the common's paving or on the
/// ground.
pub(super) fn footing(land: &Land, p: Vec2) -> f32 {
    footing_on(land, p, |p| land.surface(p))
}

/// [`footing`], with the ground's surface from `surface`.
fn footing_on(
    land: &Land,
    p: Vec2,
    surface: impl FnOnce(Vec2) -> f32,
) -> f32 {
    let town = &land.set.town;
    if land.site_dist(p) < town.plaza_radius - 0.1 {
        land.set.land.site_y + town.plaza_lift
    } else {
        surface(p)
    }
}

impl<'a> Floor<'a> {
    pub(super) fn new(land: &'a Land, lots: Vec<Lot>) -> Self {
        Self { land, lots }
    }

    /// The floor as far as `half` from `middle` each way.
    pub fn near(&self, middle: Vec2, half: f32) -> NearFloor<'a> {
        let lots = self
            .lots
            .iter()
            .filter(|lot| {
                lot.origin.xz().distance(middle) < half * 1.5 + 12.0
            })
            .copied()
            .collect();
        NearFloor {
            land: self.land,
            lots,
            surfaces: Surfaces::new(self.land),
        }
    }
}

impl NearFloor<'_> {
    /// The height of the floor at `p`.
    pub fn at(&mut self, p: Vec2) -> f32 {
        let surfaces = &mut self.surfaces;
        let ground = footing_on(self.land, p, |p| surfaces.at(p));
        self.lots
            .iter()
            .filter_map(|lot| lot.paved(p))
            .fold(ground, f32::max)
    }
}

impl Lot {
    /// The height of the yard's paving at `p`, where it is paved.
    fn paved(&self, p: Vec2) -> Option<f32> {
        let local = self.local(Vec3::new(p.x, self.origin.y, p.y));
        self.yard
            .holds(local)
            .then_some(self.origin.y + self.yard.shape.terrace)
    }

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
