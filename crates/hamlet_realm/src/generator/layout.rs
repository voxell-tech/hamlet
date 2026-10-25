//! The places of things in a town and the run of a road between two
//! doors. A plot depends only on the seed, its site and its number; a
//! route only on the two plots it joins.

use std::f32::consts::TAU;

use bevy::prelude::*;

use super::land::Land;
use super::noise::{key, signed};
use crate::meshes::Line;

/// The layout of a town.
#[derive(Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TownSettings {
    /// Radius inside which a town builds and the ground is gentled.
    pub radius: f32,
    /// Distance from the centre at which roads join a corridor.
    pub gate_radius: f32,
    /// Plots a town has, the hall's included.
    pub plots: u32,
    pub plaza_radius: f32,
    /// Height of the common's paving above the ground.
    pub plaza_lift: f32,
    /// Distance of the hall from the centre.
    pub hall_out: f32,
    /// Distance of the first ring of plots from the centre.
    pub first_ring: f32,
    /// Ground a plot takes, garden included.
    pub plot_area: f32,
    /// Distance from a house's middle to where its roads end.
    pub door_out: f32,
    /// Room kept clear around a house's middle for the house itself.
    pub house_radius: f32,
    /// Room a road keeps from whatever it goes round.
    pub road_clearance: f32,
    /// Room a robot crossing open ground keeps from whatever it goes
    /// round.
    pub robot_clearance: f32,
    /// The workbench and the board on the common, in the town's
    /// frame, each with the radius roads keep clear.
    pub bench: (Vec2, f32),
    pub board: (Vec2, f32),
    /// The place the common's lantern hangs at, in the town's frame.
    pub lamp: Vec3,
    /// The least distance between the middle of a town that is set
    /// down and the middle of any other.
    pub keep_apart: f32,
}

/// The plot of a town's first mayor, on the common.
pub const HALL_PLOT: u32 = 0;
/// A place for one house.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plot {
    pub at: Vec2,
    /// The way the door faces, as a rotation about Y from +Z.
    pub yaw: f32,
    /// False where the ground is wet or too far out.
    pub usable: bool,
    /// Distance from its middle to where its roads end.
    pub door_out: f32,
}

impl Plot {
    pub fn facing(&self) -> Vec2 {
        Vec2::new(self.yaw.sin(), self.yaw.cos())
    }

    pub fn door(&self) -> Vec2 {
        self.at + self.facing() * self.door_out
    }

    /// The spot the house's robot stands at when it has nothing to
    /// do.
    pub(crate) fn porch(&self) -> Vec2 {
        self.at
            + self.facing() * (self.door_out - 0.5)
            + self.facing().perp() * 1.45
    }
}

/// A point of the town at `site`, given in the town's own frame.
pub(crate) fn in_town(land: &Land, site: usize, local: Vec2) -> Vec2 {
    let s = &land.sites[site];
    s.at + Vec2::from_angle(s.turn).rotate(local)
}

/// Plot `k` of the town at `site`. Plots wind outward on a sunflower
/// spiral, so a town fills from the middle and stays round.
pub fn plot(land: &Land, site: usize, k: u32) -> Plot {
    let seed = land.seed;
    let town = &land.set.town;
    let door_out = town.door_out;
    let hall = in_town(land, site, Vec2::new(0.0, -town.hall_out));
    if k == HALL_PLOT {
        let f = (land.sites[site].at - hall).normalize();
        return Plot {
            at: hall,
            yaw: f.x.atan2(f.y),
            usable: true,
            door_out,
        };
    }
    let j = (k - 1) as f32;
    let (sk, kk) = (site as u32, k);
    let r = (town.first_ring * town.first_ring
        + j * town.plot_area / TAU * 2.0)
        .sqrt();
    let a = j * 2.399_963 + 0.6;
    let jitter = Vec2::new(
        signed(key(&[seed, 31, sk, kk])),
        signed(key(&[seed, 32, sk, kk])),
    ) * 0.45;
    let at = in_town(land, site, Vec2::from_angle(a) * r + jitter);
    // Doors look toward the hall, each a little off.
    let f = (hall - at).normalize();
    let yaw = f.x.atan2(f.y) + signed(key(&[seed, 33, sk, kk])) * 0.3;
    let dry = [Vec2::ZERO, Vec2::X, -Vec2::X, Vec2::Y, -Vec2::Y]
        .iter()
        .all(|&d| {
            land.height(at + d * 3.0) > land.set.land.water_y + 0.7
        });
    Plot {
        at,
        yaw,
        usable: dry && r < town.radius - 1.5,
        door_out,
    }
}

/// Things on the common that roads go round: the workbench and the
/// board, each with its radius.
pub(crate) fn common_fixtures(
    land: &Land,
    site: usize,
) -> [(Vec2, f32); 2] {
    let town = &land.set.town;
    [town.bench, town.board]
        .map(|(at, radius)| (in_town(land, site, at), radius))
}

/// Everything a road through the town at `site` must go round.
fn obstacles(land: &Land, site: usize) -> Vec<(Vec2, f32)> {
    let town = &land.set.town;
    let clear = town.road_clearance;
    let mut out = (0..town.plots)
        .map(|k| (plot(land, site, k).at, town.house_radius + clear))
        .collect::<Vec<_>>();
    out.extend(
        common_fixtures(land, site)
            .iter()
            .map(|&(p, r)| (p, r + clear)),
    );
    out
}

/// A way from the end of `head` to the start of `tail` that bends
/// round `obstacles`. The points of `head` and `tail` stay put.
pub(super) fn dodge(
    head: &[Vec2],
    tail: &[Vec2],
    obstacles: &[(Vec2, f32)],
) -> Vec<Vec2> {
    let (from, to) = (*head.last().unwrap(), tail[0]);
    let n = (from.distance(to) / 1.5).ceil().max(2.0) as usize;
    let mut pts = head.to_vec();
    pts.extend((1..n).map(|i| from.lerp(to, i as f32 / n as f32)));
    pts.extend_from_slice(tail);
    let free = head.len()..pts.len() - tail.len();
    let along = (to - from).normalize_or_zero();
    let push = |pts: &mut Vec<Vec2>| {
        for i in free.clone() {
            for &(c, r) in obstacles {
                let d = pts[i] - c;
                let len = d.length();
                if len < r {
                    // A point dead on the middle leaves to one side.
                    let out = if len > 1e-3 {
                        d / len
                    } else {
                        along.perp()
                    };
                    pts[i] = c + out * r;
                }
            }
        }
    };
    for _ in 0..60 {
        push(&mut pts);
        for i in free.clone() {
            let mid = (pts[i - 1] + pts[i + 1]) * 0.5;
            pts[i] = pts[i].lerp(mid, 0.5);
        }
    }
    for _ in 0..3 {
        push(&mut pts);
    }
    pts
}

/// The two fixed points a road starts with at a door: the door, and
/// a step straight out from it.
fn stem(land: &Land, p: &Plot) -> [Vec2; 2] {
    let town = &land.set.town;
    let out = town.house_radius + town.road_clearance + 0.35;
    [p.door(), p.at + p.facing() * out]
}

/// One end of a road: a plot in the town at a site.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord,
)]
pub struct End {
    pub site: usize,
    pub plot: u32,
}

/// A road's line over the ground.
pub struct Route {
    pub line: Line,
    /// Height of the road's surface at each point of `line`.
    pub y: Vec<f32>,
    /// Height of each point above the ground: above zero on bridges
    /// and their ramps.
    pub raised: Vec<f32>,
    /// True where the common's paving covers the road.
    pub on_plaza: Vec<bool>,
}

impl Route {
    /// The route between two ends. The order of the ends does not
    /// matter.
    pub fn between(land: &Land, a: End, b: End) -> Self {
        let (a, b) = if a <= b { (a, b) } else { (b, a) };
        let (pa, pb) =
            (plot(land, a.site, a.plot), plot(land, b.site, b.plot));
        let (sa, mut sb) = (stem(land, &pa), stem(land, &pb));
        sb.reverse();
        let way = if a.site == b.site {
            let mut round = obstacles(land, a.site);
            // Only a road to the hall comes on to the common. One
            // between two houses goes round it.
            if a.plot != HALL_PLOT && b.plot != HALL_PLOT {
                let town = &land.set.town;
                round.push((
                    land.sites[a.site].at,
                    // The smoothing of the line cuts a little in.
                    town.plaza_radius + town.road_clearance * 1.8,
                ));
            }
            dodge(&sa, &sb, &round)
        } else {
            let mid = land.corridor(a.site, b.site);
            let (first, last) = (mid[0], *mid.last().unwrap());
            let mut way =
                dodge(&sa, &[first], &obstacles(land, a.site));
            way.extend_from_slice(&mid[1..mid.len() - 1]);
            way.extend(dodge(&[last], &sb, &obstacles(land, b.site)));
            way
        };
        Self::over(land, &way, [a.site, b.site])
    }

    /// The road along `way`, which ends at the commons of `ends`.
    fn over(land: &Land, way: &[Vec2], ends: [usize; 2]) -> Self {
        let (water, town) = (land.set.land.water_y, &land.set.town);
        let line = Line::spline(way, 0.5);
        let at = |i: usize| Vec2::new(line.pts[i].x, line.pts[i].z);
        let n = line.pts.len();
        let ground =
            (0..n).map(|i| land.surface(at(i))).collect::<Vec<_>>();
        let wet = ground
            .iter()
            .map(|&g| g < water + 0.3)
            .collect::<Vec<_>>();
        let mut y = ground.clone();
        // A deck over water, eased down to the banks on both sides.
        if wet.contains(&true) {
            let mut near = vec![false; n];
            for i in (0..n).filter(|&i| wet[i]) {
                y[i] = water + land.set.road.deck;
                for slot in near
                    .iter_mut()
                    .take((i + 24).min(n))
                    .skip(i.saturating_sub(24))
                {
                    *slot = true;
                }
            }
            for _ in 0..80 {
                for i in 1..n - 1 {
                    if near[i] && !wet[i] {
                        let mid = (y[i - 1] + y[i + 1]) * 0.5;
                        y[i] = ground[i].max(mid);
                    }
                }
            }
        }
        let raised =
            (0..n).map(|i| y[i] - ground[i]).collect::<Vec<_>>();
        // Only the towns it joins have paving for it to run under.
        // A site it passes on the way, as a road to a town that was
        // moved may, is open ground.
        let on_plaza = (0..n)
            .map(|i| {
                ends.iter().any(|&site| {
                    land.sites[site].at.distance(at(i))
                        < town.plaza_radius - 0.25
                })
            })
            .collect::<Vec<_>>();
        for i in 0..n {
            if on_plaza[i] {
                y[i] = land.set.land.site_y + town.plaza_lift;
            }
        }
        Self {
            line,
            y,
            raised,
            on_plaza,
        }
    }

    pub fn total(&self) -> f32 {
        self.line.total()
    }

    /// The point on the road's surface at arc length `s`, and the
    /// direction of travel.
    pub fn at(&self, s: f32) -> (Vec3, Vec3) {
        let (p, tan) = self.line.at(s);
        let i = self
            .line
            .len
            .partition_point(|&l| l < s)
            .clamp(1, self.y.len() - 1);
        let span =
            (self.line.len[i] - self.line.len[i - 1]).max(1e-6);
        let k = ((s - self.line.len[i - 1]) / span).clamp(0.0, 1.0);
        let y = self.y[i - 1] + (self.y[i] - self.y[i - 1]) * k;
        (Vec3::new(p.x, y, p.z), tan)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generator::testing;

    #[test]
    fn a_road_goes_round_the_houses_and_only_one_to_the_hall_crosses_the_common()
     {
        let land = testing::land(1);
        let (middle, plaza) =
            (land.sites[0].at, land.set.town.plaza_radius);
        let nearest = |a: u32, b: u32| {
            let (ea, eb) =
                (End { site: 0, plot: a }, End { site: 0, plot: b });
            let route = Route::between(&land, ea, eb);
            // Going round the common takes it through no house.
            for k in
                (0..land.set.town.plots).filter(|&k| k != a && k != b)
            {
                let c = plot(&land, 0, k).at;
                let (d, _) = route.line.nearest(c.x, c.y);
                assert!(
                    d > land.set.town.house_radius,
                    "road {a}-{b} at {d} of house {k}"
                );
            }
            route.line.nearest(middle.x, middle.y).0
        };
        // Houses across the common from each other, and others.
        for a in 1..12 {
            for b in a + 1..12 {
                let d = nearest(a, b);
                assert!(d > plaza, "road {a}-{b} at {d}");
            }
        }
        assert!(nearest(HALL_PLOT, 3) < plaza + 3.0);
        // The outer houses are gone round too.
        nearest(3, 17);
        nearest(20, 28);
    }

    #[test]
    fn a_road_to_a_moved_town_is_whole_where_it_passes_a_site() {
        let mut land = testing::land(1);
        let reach = land.set.town.plaza_radius + 2.0;
        // A town set down beyond another site, as seen from the
        // first: its road runs past that site.
        let (first, by) = (land.sites[0].at, land.sites[2].at);
        let to = by + (by - first).normalize() * 110.0;
        land.move_site(3, to);
        let route = Route::between(
            &land,
            End { site: 0, plot: 0 },
            End { site: 3, plot: 0 },
        );
        let points = (0..route.line.pts.len()).map(|i| {
            let p = route.line.pts[i];
            (Vec2::new(p.x, p.z), route.on_plaza[i])
        });
        let mut passed = false;
        for (p, paved) in points {
            let ends = p.distance(first).min(p.distance(to));
            passed |= p.distance(by) < reach;
            // Paving covers it at the commons it joins and nowhere
            // else.
            assert!(!paved || ends < reach, "cut at {p}");
        }
        assert!(passed, "the road does not pass the site");
    }
}
