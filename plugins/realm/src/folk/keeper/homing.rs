//! The way home for a robot that stands away from its house: round
//! the houses and dry all the way, by the roads and their bridges
//! where the straight way is wet.

use std::collections::HashMap;

use glam::{Vec2, Vec3, Vec3Swizzles};

use crate::build::mesh::Line;
use crate::folk::chief::Walking;
use crate::world::land::Land;
use crate::world::layout::Route;

/// Spacing of the points of a way along a road.
const ALONG: f32 = 1.5;
/// Factor by which a stretch of open ground counts longer than the
/// same stretch of road, when a road to join is chosen.
const OFF_ROAD: f32 = 4.0;
/// The roads a robot may try to join before it gives up.
const TRIES: usize = 3;

/// A way home, and whether part of it runs on a road.
pub(super) struct Homeward {
    pub way: Vec<Vec3>,
    pub by_road: bool,
}

/// The distance from `p` to the nearest point of `line`, and how far
/// along it that point is.
fn nearest(line: &Line, p: Vec2) -> (f32, f32) {
    let p = Vec3::new(p.x, 0.0, p.y);
    let mut best = (f32::MAX, 0.0);
    for i in 1..line.pts.len() {
        let (a, b) = (line.pts[i - 1], line.pts[i]);
        let ab = b - a;
        let t = ((p - a).dot(ab) / ab.length_squared().max(1e-9))
            .clamp(0.0, 1.0);
        let d = (a + ab * t - p).length_squared();
        if d < best.0 {
            best = (d, line.len[i - 1] + t * ab.length());
        }
    }
    (best.0.sqrt(), best.1)
}

/// A crossing of roads, as a key.
fn node(p: Vec3) -> (i32, i32) {
    ((p.x * 2.0).round() as i32, (p.z * 2.0).round() as i32)
}

/// The two ends of a road, with how far along it each is.
fn ends(route: &Route) -> [(Vec3, f32); 2] {
    let pts = &route.line.pts;
    [(pts[0], 0.0), (pts[pts.len() - 1], route.total())]
}

/// The points of `route` from `from` along it to `to`, on its
/// surface.
fn along(route: &Route, from: f32, to: f32, way: &mut Vec<Vec3>) {
    let n = ((to - from).abs() / ALONG).ceil().max(1.0) as usize;
    way.extend((0..=n).map(|i| {
        route.at(from + (to - from) * i as f32 / n as f32).0
    }));
}

/// A way by the roads from `from` to `to`, which lies in the town
/// about `town`. None if no road can be reached dry or none leads
/// there.
fn by_road(
    land: &Land,
    roads: &[&Route],
    from: Vec2,
    to: Vec2,
    town: Vec2,
    ground: &impl Fn(Vec2) -> f32,
) -> Option<Vec<Vec3>> {
    let reach = land.set.town.radius;
    // Distance of each crossing from home by the roads. A crossing in
    // the home town is left over open ground.
    let mut far = HashMap::new();
    for route in roads {
        for (end, _) in ends(route) {
            if end.xz().distance(town) < reach {
                far.insert(node(end), end.xz().distance(to));
            }
        }
    }
    for _ in 0..roads.len() {
        let mut changed = false;
        for route in roads {
            let [(a, _), (b, total)] = ends(route);
            for (here, there) in [(a, b), (b, a)] {
                let Some(&known) = far.get(&node(there)) else {
                    continue;
                };
                let next = known + total;
                if far
                    .get(&node(here))
                    .is_none_or(|&d| d > next + 1e-3)
                {
                    far.insert(node(here), next);
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }

    // The roads it could join, nearest way home first.
    let mut joins = roads
        .iter()
        .filter_map(|route| {
            let (off, at) = nearest(&route.line, from);
            ends(route)
                .iter()
                .filter_map(|&(end, s)| {
                    let home = far.get(&node(end))?;
                    Some((
                        OFF_ROAD * off + (s - at).abs() + home,
                        end,
                        s,
                    ))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(cost, end, s)| (cost, *route, at, end, s))
        })
        .collect::<Vec<_>>();
    joins.sort_by(|a, b| a.0.total_cmp(&b.0));

    for (_, route, at, end, s) in joins.into_iter().take(TRIES) {
        let on = route.at(at).0;
        let Some(hop) = land.overland(from, on.xz()) else {
            continue;
        };
        let mut way = hop
            .iter()
            .map(|&p| p.extend(ground(p)).xzy())
            .collect::<Vec<_>>();
        way.pop();
        along(route, at, s, &mut way);
        // From crossing to crossing, each nearer home than the last.
        let mut here = end;
        for _ in 0..roads.len() {
            let left = far[&node(here)];
            let next = roads.iter().find_map(|next| {
                let [(a, _), (b, total)] = ends(next);
                [(a, b, 0.0, total), (b, a, total, 0.0)]
                    .into_iter()
                    .find(|&(near, other, ..)| {
                        node(near) == node(here)
                            && far.get(&node(other)).is_some_and(
                                |&d| d + total <= left + 1e-2,
                            )
                    })
                    .map(|(_, other, s0, s1)| (*next, other, s0, s1))
            });
            let Some((next, other, s0, s1)) = next else {
                break;
            };
            way.pop();
            along(next, s0, s1, &mut way);
            here = other;
        }
        let Some(last) = land.overland(here.xz(), to) else {
            continue;
        };
        way.pop();
        way.extend(last.iter().map(|&p| p.extend(ground(p)).xzy()));
        return Some(way);
    }
    None
}

/// The way from `from` to `to`, a point by the house whose middle
/// is `home`. `ground` gives the height a robot stands at. None if
/// there is no dry way.
pub(super) fn homeward(
    land: &Land,
    roads: &[&Route],
    from: Vec2,
    to: Vec2,
    home: Vec2,
    ground: impl Fn(Vec2) -> f32,
) -> Option<Homeward> {
    if let Some(way) = land.overland(from, to) {
        return Some(Homeward {
            way: way
                .into_iter()
                .map(|p| p.extend(ground(p)).xzy())
                .collect(),
            by_road: false,
        });
    }
    let town = land.sites.iter().map(|s| s.at).min_by(|a, b| {
        a.distance(home).total_cmp(&b.distance(home))
    })?;
    by_road(land, roads, from, to, town, &ground)
        .map(|way| Homeward { way, by_road: true })
}
