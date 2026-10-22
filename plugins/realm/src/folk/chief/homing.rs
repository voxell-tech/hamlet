//! The mascot's way on foot: a spot clear of the houses to stand at,
//! and a dry way over open ground round whatever stands between.

use std::f32::consts::PI;

use glam::Vec2;

use crate::world::land::Land;
use crate::world::layout::{common_fixtures, dodge, plot};

/// Spacing at which a way is tested for water.
const TEST: f32 = 0.75;
/// Farthest off its line a point of a way may be and be left out.
const SLACK: f32 = 0.06;

/// The mascot's ways over the land.
pub trait Walking {
    /// True when a walker may stand at `p`: on dry ground, clear of
    /// whatever it goes round.
    fn clear(&self, p: Vec2) -> bool;

    /// The spot to stand at by `them`: at `side` of it, or as near
    /// that as is clear, first turned about `them` and then further
    /// out, up to `reach`. None if no spot is clear.
    fn beside(
        &self,
        them: Vec2,
        side: Vec2,
        reach: f32,
    ) -> Option<Vec2>;

    /// A dry way over open ground from `from` to `to`, round whatever
    /// stands between. None if there is none this simply.
    fn overland(&self, from: Vec2, to: Vec2) -> Option<Vec<Vec2>>;

    /// Everything a walker from `a` to `b` goes round: the plots and
    /// fixtures of the towns it passes, and held ground.
    fn rounds(&self, a: Vec2, b: Vec2) -> Vec<(Vec2, f32)>;

    /// True when any of the ground under `way` is water.
    fn wet(&self, way: &[Vec2]) -> bool;
}

impl Walking for Land {
    fn clear(&self, p: Vec2) -> bool {
        !self.is_water(p)
            && self
                .rounds(p, p)
                .iter()
                .all(|&(c, r)| p.distance(c) >= r)
    }

    fn beside(
        &self,
        them: Vec2,
        side: Vec2,
        reach: f32,
    ) -> Option<Vec2> {
        // Half turns, nearest first.
        const TURNS: [f32; 8] =
            [0.0, 0.25, -0.25, 0.5, -0.5, 0.75, -0.75, 1.0];
        let rings =
            (reach / side.length().max(0.1)).max(1.0) as usize;
        (1..=rings)
            .flat_map(|ring| {
                TURNS.iter().map(move |turn| {
                    them + Vec2::from_angle(turn * PI).rotate(side)
                        * ring as f32
                })
            })
            .find(|&p| self.clear(p))
    }

    fn overland(&self, from: Vec2, to: Vec2) -> Option<Vec<Vec2>> {
        if from.distance(to) < 0.05 {
            return Some(vec![from, to]);
        }
        let mut way = dodge(&[from], &[to], &self.rounds(from, to));
        thin(&mut way);
        (!self.wet(&way)).then_some(way)
    }

    fn rounds(&self, a: Vec2, b: Vec2) -> Vec<(Vec2, f32)> {
        let town = &self.set.town;
        let keep = town.house_radius + town.robot_clearance;
        let ab = b - a;
        let mut out = Vec::new();
        for (site, at) in self.sites.iter().enumerate() {
            let t = ((at.at - a).dot(ab)
                / ab.length_squared().max(1e-6))
            .clamp(0.0, 1.0);
            if (a + ab * t).distance(at.at) > town.radius + keep {
                continue;
            }
            out.extend(
                (0..town.plots)
                    .map(|k| plot(self, site, k))
                    .filter(|p| p.usable)
                    .map(|p| (p.at, keep)),
            );
            out.extend(
                common_fixtures(self, site)
                    .iter()
                    .map(|&(p, r)| (p, r + town.robot_clearance)),
            );
        }
        out.extend(
            self.held()
                .into_iter()
                .map(|(p, r)| (p, r + town.robot_clearance)),
        );
        out
    }

    fn wet(&self, way: &[Vec2]) -> bool {
        way.windows(2).any(|w| {
            let n =
                (w[0].distance(w[1]) / TEST).ceil().max(1.0) as usize;
            (0..=n).any(|i| {
                self.is_water(w[0].lerp(w[1], i as f32 / n as f32))
            })
        })
    }
}

/// Leaves out the points of `way` that lie on the line between
/// their neighbours.
fn thin(way: &mut Vec<Vec2>) {
    let mut kept = 0;
    for i in 1..way.len() {
        let last = i + 1 == way.len();
        let on_line = !last && {
            let (a, b) = (way[kept], way[i + 1]);
            let ab = b - a;
            let t = ((way[i] - a).dot(ab)
                / ab.length_squared().max(1e-6))
            .clamp(0.0, 1.0);
            (a + ab * t).distance(way[i]) < SLACK
        };
        if !on_line {
            kept += 1;
            way[kept] = way[i];
        }
    }
    way.truncate(kept + 1);
}
