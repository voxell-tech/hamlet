//! The way home for a robot that stands away from its house: round
//! the houses and dry all the way, by the roads and their bridges
//! where the straight way is wet.

use std::collections::HashMap;
use std::f32::consts::PI;

use bevy::prelude::*;

use super::land::Land;
use super::layout::{Route, common_fixtures, dodge, plot};

/// Spacing at which a way is tested for water.
const TEST: f32 = 0.75;
/// Spacing of the points of a way along a road.
const ALONG: f32 = 1.5;
/// Farthest off its line a point of a way may be and be left out.
const SLACK: f32 = 0.06;
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

/// Everything a robot walking from `a` to `b` goes round: the plots
/// and fixtures of the towns it passes, and held ground.
fn rounds(land: &Land, a: Vec2, b: Vec2) -> Vec<(Vec2, f32)> {
    let town = &land.set.town;
    let keep = town.house_radius + town.robot_clearance;
    let ab = b - a;
    let mut out = Vec::new();
    for (site, at) in land.sites.iter().enumerate() {
        let t = ((at.at - a).dot(ab) / ab.length_squared().max(1e-6))
            .clamp(0.0, 1.0);
        if (a + ab * t).distance(at.at) > town.radius + keep {
            continue;
        }
        out.extend(
            (0..town.plots)
                .map(|k| plot(land, site, k))
                .filter(|p| p.usable)
                .map(|p| (p.at, keep)),
        );
        out.extend(
            common_fixtures(land, site)
                .iter()
                .map(|&(p, r)| (p, r + town.robot_clearance)),
        );
    }
    out.extend(
        land.held()
            .into_iter()
            .map(|(p, r)| (p, r + town.robot_clearance)),
    );
    out
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

/// True when any of the ground under `way` is water.
fn wet(land: &Land, way: &[Vec2]) -> bool {
    way.windows(2).any(|w| {
        let n = (w[0].distance(w[1]) / TEST).ceil().max(1.0) as usize;
        (0..=n).any(|i| {
            land.is_water(w[0].lerp(w[1], i as f32 / n as f32))
        })
    })
}

/// True when a walker may stand at `p`: on dry ground, clear of
/// whatever it goes round.
pub(crate) fn clear(land: &Land, p: Vec2) -> bool {
    !land.is_water(p)
        && rounds(land, p, p).iter().all(|&(c, r)| p.distance(c) >= r)
}

/// The spot to stand at by `them`: at `side` of it, or as near that
/// as is clear, first turned about `them` and then further out, up to
/// `reach`. None if no spot is clear.
pub(crate) fn beside(
    land: &Land,
    them: Vec2,
    side: Vec2,
    reach: f32,
) -> Option<Vec2> {
    // Half turns, nearest first.
    const TURNS: [f32; 8] =
        [0.0, 0.25, -0.25, 0.5, -0.5, 0.75, -0.75, 1.0];
    let rings = (reach / side.length().max(0.1)).max(1.0) as usize;
    (1..=rings)
        .flat_map(|ring| {
            TURNS.iter().map(move |turn| {
                them + Vec2::from_angle(turn * PI).rotate(side)
                    * ring as f32
            })
        })
        .find(|&p| clear(land, p))
}

/// A dry way over open ground from `from` to `to`, round whatever
/// stands between. None if there is none this simply.
pub(crate) fn overland(
    land: &Land,
    from: Vec2,
    to: Vec2,
) -> Option<Vec<Vec2>> {
    if from.distance(to) < 0.05 {
        return Some(vec![from, to]);
    }
    let mut way = dodge(&[from], &[to], &rounds(land, from, to));
    thin(&mut way);
    (!wet(land, &way)).then_some(way)
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
            let (off, at) = route.line.nearest(from.x, from.y);
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
        let Some(hop) = overland(land, from, on.xz()) else {
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
        let Some(last) = overland(land, here.xz(), to) else {
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
    if let Some(way) = overland(land, from, to) {
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

#[cfg(test)]
mod tests {
    use super::super::layout::End;
    use super::*;
    use crate::generator::testing;

    #[test]
    fn a_robot_across_the_river_comes_home_dry_and_round_the_houses()
    {
        let land = testing::land(7);
        let town = &land.set.town;
        // A road between two towns that has a bridge on it.
        let (near, far) = (0..land.sites.len())
            .flat_map(|a| (0..a).map(move |b| (a, b)))
            .find(|&(a, b)| {
                land.sites[a].at.distance(land.sites[b].at) < 160.0
                    && Route::between(
                        &land,
                        End { site: a, plot: 3 },
                        End { site: b, plot: 3 },
                    )
                    .raised
                    .iter()
                    .any(|&r| r > 0.3)
            })
            .expect("two towns with a river between them");
        let road = Route::between(
            &land,
            End {
                site: near,
                plot: 3,
            },
            End { site: far, plot: 3 },
        );
        let house = plot(&land, near, 3);
        let other = plot(&land, far, 3);
        let keep = town.house_radius + town.robot_clearance;
        let to = house.at + house.facing() * keep;
        // Set down behind the house at the far end of the road.
        let from = other.at - other.facing() * (keep + 1.0);
        assert!(overland(&land, from, to).is_none());

        let home =
            homeward(&land, &[&road], from, to, house.at, |p| {
                land.surface(p)
            })
            .expect("a way home");
        assert!(home.by_road);
        let off_road = |p: Vec2| road.line.nearest(p.x, p.y).0;
        let on_road = |p: Vec2| off_road(p) < 0.3;
        for w in home.way.windows(2) {
            for i in 0..=8 {
                let p = w[0].lerp(w[1], i as f32 / 8.0).xz();
                assert!(
                    !land.is_water(p) || on_road(p),
                    "wet at {p}"
                );
                for k in 0..town.plots {
                    for site in [near, far] {
                        let at = plot(&land, site, k).at;
                        assert!(
                            p.distance(at) > town.house_radius
                                // A road ends at a door.
                                || off_road(p) < 1.2,
                            "through plot {k} at {p}"
                        );
                    }
                }
            }
        }
        // With no road there is no way.
        assert!(
            homeward(&land, &[], from, to, house.at, |p| land
                .surface(p))
            .is_none()
        );
    }

    #[test]
    fn a_spot_beside_someone_at_the_water_is_dry_and_clear() {
        let land = testing::land(7);
        let side = Vec2::new(1.0, 0.25);
        // On a bank, with the water to that side.
        let them = (0..4000)
            .map(|i| {
                Vec2::new((i % 80) as f32, (i / 80) as f32) * 2.0
                    - Vec2::splat(80.0)
            })
            .find(|&p| clear(&land, p) && land.is_water(p + side))
            .expect("a bank");
        let stand = beside(&land, them, side, 6.0).expect("a spot");
        assert!(clear(&land, stand));
        assert!(stand.distance(them) <= 6.0);
    }

    /// Prints what finding a way costs. Run with
    /// `cargo test --release homing::tests::cost -- --ignored
    /// --nocapture`.
    #[test]
    #[ignore]
    fn cost() {
        let land = testing::land(7);
        let roads = (1..30)
            .map(|k| {
                Route::between(
                    &land,
                    End { site: 0, plot: 0 },
                    End { site: 0, plot: k },
                )
            })
            .chain((1..6).map(|site| {
                Route::between(
                    &land,
                    End { site: 0, plot: 0 },
                    End { site, plot: 0 },
                )
            }))
            .collect::<Vec<_>>();
        let roads = roads.iter().collect::<Vec<_>>();
        let house = plot(&land, 0, 3);
        let to = house.at + house.facing() * 3.0;
        for (name, from) in [
            (
                "across the town",
                plot(&land, 0, 20).at + Vec2::X * 4.0,
            ),
            ("from the next town", land.sites[1].at + Vec2::X * 9.0),
        ] {
            const N: u32 = 200;
            let started = std::time::Instant::now();
            let mut points = 0;
            for _ in 0..N {
                points = homeward(
                    &land,
                    &roads,
                    from,
                    to,
                    house.at,
                    |p| land.surface(p),
                )
                .map_or(0, |h| h.way.len());
            }
            let each = started.elapsed().as_micros() / N as u128;
            println!("{name}: {each} microseconds, {points} points");
        }
    }
}
