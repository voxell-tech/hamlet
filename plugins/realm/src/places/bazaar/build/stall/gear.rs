//! Cogs that mesh: every cog that turns another shares its tooth
//! size, stands the sum of their pitch radii from it, and starts
//! turned so that a gap of it meets a tooth of the other.

use std::f32::consts::{PI, TAU};

use glam::{Quat, Vec2, Vec3};

use crate::build::mesh::Mb;
use crate::places::bazaar::build::Shapes;

/// How far a tooth stands out past the pitch circle, in modules.
const ADDENDUM: f32 = 0.75;
/// How far a tooth reaches in below it, into the rim, in modules.
const ROOT: f32 = 1.1;
/// Half a tooth's width across, in modules. A gap is `PI` wide, so a
/// tooth of the other cog has room.
const HALF_WIDTH: f32 = 0.55;
/// The rim's tube, and its gap below the other cog's tips, in
/// modules.
const RIM: f32 = 0.3;
const CLEAR: f32 = 0.1;

/// A cog's teeth: how many, and their size, the module, which is
/// the pitch radius over half the teeth.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gear {
    pub teeth: usize,
    pub module: f32,
}

impl Gear {
    /// A cog of about `radius` with `teeth` teeth.
    pub fn sized(radius: f32, teeth: usize) -> Gear {
        Gear {
            teeth,
            module: 2.0 * radius / teeth as f32,
        }
    }

    /// The cog that meshes with this one, with `teeth` teeth.
    pub fn mate(self, teeth: usize) -> Gear {
        Gear { teeth, ..self }
    }

    /// The radius at which it meets a cog it meshes with.
    pub fn pitch(self) -> f32 {
        self.module * self.teeth as f32 / 2.0
    }

    /// The centre of the rim's tube.
    fn rim(self) -> f32 {
        self.pitch() - (ADDENDUM + CLEAR + RIM) * self.module
    }

    /// The rim's outer edge.
    #[cfg(test)]
    fn rim_out(self) -> f32 {
        self.rim() + RIM * self.module
    }

    /// The tips of its teeth.
    #[cfg(test)]
    fn tip(self) -> f32 {
        self.pitch() + ADDENDUM * self.module
    }

    /// The teeth's middle, radially, and their half sizes, radially
    /// and across.
    fn tooth(self) -> (f32, Vec2) {
        let m = self.module;
        let middle = self.pitch() + (ADDENDUM - ROOT) * m / 2.0;
        (
            middle,
            Vec2::new((ADDENDUM + ROOT) * m / 2.0, HALF_WIDTH * m),
        )
    }

    /// The speed of the cog meshing with this one, as `mate`, when
    /// this one turns at `rate`.
    pub fn driven(self, mate: Gear, rate: f32) -> f32 {
        -rate * self.teeth as f32 / mate.teeth as f32
    }

    /// Where the cog `mate` stands, beside this one at `at`, toward
    /// `toward`.
    pub fn beside(self, mate: Gear, at: Vec3, toward: Vec2) -> Vec3 {
        let gap = toward.normalize_or(Vec2::X)
            * (self.pitch() + mate.pitch());
        at + gap.extend(0.0)
    }

    /// The turn `mate`, at `mate_at`, starts at so that one of its
    /// gaps meets a tooth of this one, at `at`, turned `phase`. The
    /// two stay in mesh as they turn at speeds of [`Gear::driven`].
    pub fn phase_of(
        self,
        mate: Gear,
        (at, phase): (Vec3, f32),
        mate_at: Vec3,
    ) -> f32 {
        let line = (mate_at - at).truncate();
        let theta = line.y.atan2(line.x);
        let (a, b) = (self.teeth as f32, mate.teeth as f32);
        // The teeth of each, measured from the line between their
        // middles, sum to half a turn of a tooth: a tooth in a gap.
        (theta + PI + (PI - (phase - theta) * a) / b).rem_euclid(TAU)
    }
}

/// A cog about an axle along Z: its rim with its teeth, its spokes
/// and its hub.
pub fn cog(mb: &mut Mb, gear: Gear, colour: Vec3) {
    let thick = 0.04 + gear.pitch() * 0.06;
    let m = gear.module;
    mb.torus(Vec3::ZERO, Vec3::Z, gear.rim(), RIM * m, 20, colour);
    let (middle, half) = gear.tooth();
    for k in 0..gear.teeth {
        let a = k as f32 / gear.teeth as f32 * TAU;
        let out = Vec3::new(a.cos(), a.sin(), 0.0);
        mb.rbox(
            out * middle,
            half.extend(thick),
            0.01,
            Quat::from_rotation_z(a),
            colour,
            0.0,
        );
    }
    for k in 0..4 {
        let a = k as f32 / 4.0 * TAU + 0.4;
        mb.tube(
            Vec3::ZERO,
            Vec3::new(a.cos(), a.sin(), 0.0) * gear.rim(),
            gear.pitch() * 0.06,
            gear.pitch() * 0.05,
            5,
            colour * 0.85,
            false,
        );
    }
    mb.rod(
        Vec3::Z * -thick * 1.4,
        Vec3::Z * thick * 1.4,
        gear.pitch() * 0.18,
        10,
        colour * 0.7,
        true,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The corners of tooth `k` of `gear` at `at`, turned `turn`.
    fn tooth(gear: Gear, at: Vec2, turn: f32, k: usize) -> [Vec2; 4] {
        let (middle, half) = gear.tooth();
        let a = turn + k as f32 / gear.teeth as f32 * TAU;
        let (out, across) =
            (Vec2::from_angle(a), Vec2::from_angle(a + PI / 2.0));
        let c = at + out * middle;
        [(1.0, 1.0), (1.0, -1.0), (-1.0, -1.0), (-1.0, 1.0)]
            .map(|(r, s)| c + out * half.x * r + across * half.y * s)
    }

    /// True when two convex shapes overlap, by their separating axes.
    fn overlap(one: &[Vec2; 4], other: &[Vec2; 4]) -> bool {
        let axes = [one, other].into_iter().flat_map(|shape| {
            (0..4).map(move |i| {
                let edge = shape[(i + 1) % 4] - shape[i];
                Vec2::new(-edge.y, edge.x)
            })
        });
        for axis in axes {
            let span = |shape: &[Vec2; 4]| {
                shape
                    .iter()
                    .map(|p| p.dot(axis))
                    .fold((f32::MAX, f32::MIN), |(lo, hi), d| {
                        (lo.min(d), hi.max(d))
                    })
            };
            let ((a0, a1), (b0, b1)) = (span(one), span(other));
            if a1 < b0 || b1 < a0 {
                return false;
            }
        }
        true
    }

    #[test]
    fn cogs_that_mesh_never_cut_into_each_other_as_they_turn() {
        for (teeth, mate_teeth, toward) in [
            (12, 7, Vec2::new(-0.72, -0.22)),
            (9, 9, Vec2::new(1.0, 0.3)),
            (16, 5, Vec2::new(0.0, -1.0)),
        ] {
            let big = Gear::sized(0.5, teeth);
            let small = big.mate(mate_teeth);
            let at = Vec3::new(0.3, 2.0, 0.25);
            let phase = 0.37;
            let mate_at = big.beside(small, at, toward);
            let mate_phase =
                big.phase_of(small, (at, phase), mate_at);
            let rate = 0.16;
            let mate_rate = big.driven(small, rate);
            let (a, b) = (at.truncate(), mate_at.truncate());
            // Round its whole turn, finely enough to catch a tooth
            // passing through another.
            for step in 0..2000 {
                let t = step as f32 / 2000.0 / rate;
                let turn = phase + TAU * rate * t;
                let mate_turn = mate_phase + TAU * mate_rate * t;
                for i in 0..big.teeth {
                    let one = tooth(big, a, turn, i);
                    for j in 0..small.teeth {
                        let other = tooth(small, b, mate_turn, j);
                        assert!(
                            !overlap(&one, &other),
                            "{teeth}:{mate_teeth} teeth {i} and {j} \
                             meet at {t}"
                        );
                    }
                    // Nor does a tooth reach the other's rim.
                    for corner in one {
                        assert!(corner.distance(b) > small.rim_out());
                    }
                }
                for j in 0..small.teeth {
                    for corner in tooth(small, b, mate_turn, j) {
                        assert!(corner.distance(a) > big.rim_out());
                    }
                }
            }
            // And they do mesh: the tips reach past each other's
            // pitch circle.
            assert!(big.tip() + small.tip() > a.distance(b));
        }
    }
}
