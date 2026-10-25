//! Road meshes: a ribbon laid on the ground along a route, at one of
//! four levels of use, with stone bridges where it crosses water and
//! the groove for its light.

use bevy::prelude::*;

use super::land::Land;
use super::layout::Route;
use super::noise::{key, perlin, unit};
use super::society::RoadLevel;
use crate::meshes::{Meshing, smoothstep};
use crate::palette::Palette;

/// Roads and their bridges.
#[derive(Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoadSettings {
    /// Height above the ground at which a road counts as a bridge.
    pub bridge_from: f32,
    pub bridge_half: f32,
    /// Height of a bridge deck above the water.
    pub deck: f32,
    /// Half the width of the trodden strip at each level of use,
    /// from overgrown to rail.
    pub half_width: [f32; 4],
    /// Half the width of a paved road's flags.
    pub flags_half: f32,
    /// Width of the groove's slate bed.
    pub groove_width: f32,
    /// Ground a robot covers per second on a road.
    pub walk_speed: f32,
    /// Ground a cart covers per second on a road between towns, and
    /// the stretch at either end over which it gets going and
    /// slows.
    pub ride_speed: f32,
    pub ride_ramp: f32,
    /// Height of a cart's bed over the road.
    pub cart_bed: f32,
    /// The longest a cart takes one way, in seconds. It goes faster
    /// on a road too long for its speed.
    pub ride_time: f32,
    /// Most a stretch through the mountains counts longer than its
    /// length when a road between towns picks its way.
    pub climb: f32,
    /// The most crossings looked at for a way between two towns
    /// before the road runs straight.
    pub search: usize,
}

/// Height of a level's surface above the ground. Busier roads lie on
/// top where two share a stretch.
fn lift(level: RoadLevel, salt: u32) -> f32 {
    let base = match level {
        RoadLevel::Overgrown => 0.05,
        RoadLevel::Dirt => 0.06,
        RoadLevel::Paved => 0.078,
        RoadLevel::Rail => 0.094,
    };
    base + 0.008 * unit(key(&[salt, 61]))
}

/// One cross-section of a road.
struct Row {
    i: usize,
    s: f32,
    c: Vec3,
    /// Unit vector across the road, to the right of travel.
    across: Vec3,
    up: Vec3,
    /// Ground height at one unit to the left and right, relative to
    /// the middle.
    tilt: [f32; 2],
    bridge: bool,
    plaza: bool,
}

impl Row {
    /// A point `w` across the road and `h` above its surface.
    fn at(&self, w: f32, h: f32) -> Vec3 {
        let side = if w < 0.0 { self.tilt[0] } else { self.tilt[1] };
        self.c + self.across * w + Vec3::Y * (h + side * w.abs())
    }
}

fn rows(land: &Land, route: &Route, stride: usize) -> Vec<Row> {
    let n = route.line.pts.len();
    let bridge_from = land.set.road.bridge_from;
    let flat =
        |i: usize| route.raised[i] > bridge_from || route.on_plaza[i];
    let mut out = Vec::new();
    for i in 0..n {
        let edge = i + 1 == n
            || (i > 0 && flat(i) != flat(i - 1))
            || (i + 1 < n && flat(i) != flat(i + 1));
        if i % stride != 0 && !edge && !flat(i) {
            continue;
        }
        let p = route.line.pts[i];
        let (a, b) = (
            route.line.pts[i.saturating_sub(1)],
            route.line.pts[(i + 1).min(n - 1)],
        );
        let tan = (b - a).normalize_or_zero();
        let across = Vec3::new(-tan.z, 0.0, tan.x);
        let c = Vec3::new(p.x, route.y[i], p.z);
        let tilt = if flat(i) {
            [0.0, 0.0]
        } else {
            let g = |side: f32| {
                let q = p + across * side;
                land.surface(Vec2::new(q.x, q.z))
                    - (c.y - route.raised[i])
            };
            [g(-1.0), g(1.0)]
        };
        let rise = (route.y[(i + 1).min(n - 1)]
            - route.y[i.saturating_sub(1)])
            / (b - a).length().max(1e-3);
        let along = (tan + Vec3::Y * rise).normalize();
        let over = (across * 2.0 + Vec3::Y * (tilt[1] - tilt[0]))
            .normalize();
        let up = over.cross(along).normalize();
        out.push(Row {
            i,
            s: route.line.len[i],
            c,
            across,
            up: if up.y < 0.0 { -up } else { up },
            tilt,
            bridge: route.raised[i] > bridge_from
                && !route.on_plaza[i],
            plaza: route.on_plaza[i],
        });
    }
    out
}

/// The stretches of a road that are bridge, for the roads that meet
/// it there.
#[derive(Clone, PartialEq, Debug)]
pub struct Span {
    /// The middle of the deck, a step at a time.
    steps: Vec<[Vec3; 2]>,
    /// The ground it lies over, from its least corner to its most.
    over: Rect,
}

impl Span {
    /// The bridges of `route`. Nothing for a road that has none.
    pub fn of(land: &Land, route: &Route) -> Option<Span> {
        let from = land.set.road.bridge_from;
        let deck =
            |i: usize| route.raised[i] > from && !route.on_plaza[i];
        let at = |i: usize| {
            let p = route.line.pts[i];
            Vec3::new(p.x, route.y[i], p.z)
        };
        let steps = (1..route.line.pts.len())
            .filter(|&i| deck(i - 1) && deck(i))
            .map(|i| [at(i - 1), at(i)])
            .collect::<Vec<_>>();
        let first = steps.first()?[0].xz();
        let over = steps
            .iter()
            .flatten()
            .fold(Rect::from_corners(first, first), |over, p| {
                over.union_point(p.xz())
            });
        Some(Span { steps, over })
    }

    /// True when the two come within `reach` of each other, by the
    /// ground each lies over.
    pub(crate) fn is_near(&self, other: &Span, reach: f32) -> bool {
        !self.over.inflate(reach).intersect(other.over).is_empty()
    }

    /// A number that is another for another span.
    pub fn mark(&self) -> u32 {
        let bits = |v: f32| v.to_bits();
        let (first, last) =
            (self.steps[0][0], self.steps[self.steps.len() - 1][1]);
        key(&[
            self.steps.len() as u32,
            bits(first.x),
            bits(first.z),
            bits(last.x),
            bits(last.z),
        ])
    }

    /// True when `p` is on its deck, which is `half` wide to each
    /// side. A road that passes over or under it is not on it.
    fn is_on_deck(&self, p: Vec3, half: f32) -> bool {
        self.over.inflate(half).contains(p.xz())
            && self.steps.iter().any(|[a, b]| {
                let along = (b - a).xz();
                let t = ((p - a).xz().dot(along)
                    / along.length_squared().max(1e-6))
                .clamp(0.0, 1.0);
                let near = a.lerp(*b, t);
                near.xz().distance(p.xz()) < half
                    && (near.y - p.y).abs() < 0.6
            })
    }
}

/// The road's surface for a level of use. `salt` is the road's own
/// number, which settles its small differences. Where a bridge of
/// `met` crosses a bridge of its own, the two are one deck.
pub fn surface(
    land: &Land,
    route: &Route,
    level: RoadLevel,
    salt: u32,
    met: &[Span],
) -> Meshing {
    let pal = &land.pal;
    let mut meshing = Meshing::new();
    let lift = lift(level, salt);
    let stride = match level {
        RoadLevel::Paved => 1,
        _ => 3,
    };
    let rows = rows(land, route, stride);
    shoulder(&mut meshing, land, pal, &rows, level, lift);
    match level {
        RoadLevel::Paved => {
            flags(
                &mut meshing,
                pal,
                &rows,
                land.set.road.flags_half,
                lift,
                false,
            );
        }
        RoadLevel::Rail => rails(&mut meshing, pal, route, lift),
        _ => {}
    }
    bridges(&mut meshing, land, pal, route, lift, met);
    meshing
}

/// The trodden strip under every road: soil in the middle that fades
/// into the grass at both edges.
fn shoulder(
    meshing: &mut Meshing,
    land: &Land,
    pal: &Palette,
    rows: &[Row],
    level: RoadLevel,
    lift: f32,
) {
    let half = land.set.road.half_width[level as usize];
    let mut prev: Option<[u32; 5]> = None;
    for row in rows {
        if row.bridge || row.plaza {
            prev = None;
            continue;
        }
        let flat = Vec2::new(row.c.x, row.c.z);
        let wide =
            half * (1.0 + 0.16 * perlin(flat * 0.45, land.seed + 62));
        let grain = 0.5 + 0.5 * perlin(flat * 1.9, land.seed + 63);
        let soil = match level {
            RoadLevel::Rail => pal.gravel * (0.85 + 0.3 * grain),
            _ => pal.dirt.lerp(pal.dirt_shade, 0.2 + 0.55 * grain),
        };
        let ids = [-1.0f32, -0.55, 0.0, 0.55, 1.0].map(|k| {
            let w = k * wide;
            let p = row.c + row.across * w;
            let grass = land.grass(Vec2::new(p.x, p.z));
            let (col, h) = if k.abs() > 0.9 {
                (grass, 0.012)
            } else if level == RoadLevel::Overgrown {
                // Unused, it is whole still: dusty, and grass has
                // crept over it in patches.
                let bare = smoothstep(
                    0.1,
                    0.6,
                    perlin(flat * 0.5, land.seed + 64),
                );
                (grass.lerp(soil, 0.5 + 0.3 * bare), lift)
            } else if k == 0.0 {
                (soil.lerp(pal.dirt, 0.35), lift)
            } else {
                (soil, lift)
            };
            meshing.v(row.at(w, h), row.up, col)
        });
        if let Some(q) = prev {
            for k in 0..4 {
                meshing.quad(q[k], q[k + 1], ids[k + 1], ids[k]);
            }
        }
        prev = Some(ids);
    }
}

/// Stone flags, three across, each its own block.
fn flags(
    meshing: &mut Meshing,
    pal: &Palette,
    rows: &[Row],
    half: f32,
    lift: f32,
    on_bridge: bool,
) {
    let h = lift + 0.012;
    let bands = [(-half, -0.21), (-0.21, 0.21), (0.21, half)];
    for w in rows.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        if a.plaza || b.plaza || (a.bridge && b.bridge) != on_bridge {
            continue;
        }
        if !on_bridge && (a.bridge || b.bridge) {
            continue;
        }
        for (k, &(w0, w1)) in bands.iter().enumerate() {
            // Coloured by place, so two roads on one stretch agree.
            let cell = key(&[
                (a.c.x * 2.0).round() as i32 as u32,
                (a.c.z * 2.0).round() as i32 as u32,
                k as u32,
            ]);
            let col = pal
                .limestone
                .lerp(pal.rock, 0.45)
                .lerp(pal.limestone_shade, unit(cell) * 0.5)
                * (0.92 + 0.16 * unit(key(&[cell, 1])));
            let gap = 0.045;
            let (p0, p1) = (a.at(w0 + gap, h), a.at(w1 - gap, h));
            let (q0, q1) = (b.at(w0 + gap, h), b.at(w1 - gap, h));
            let inset = 0.07;
            meshing.face(
                [
                    p0.lerp(q0, inset),
                    p1.lerp(q1, inset),
                    q1.lerp(p1, inset),
                    q0.lerp(p0, inset),
                ],
                a.up,
                col,
            );
        }
    }
}

/// Sleepers on the ballast and two rails over them.
fn rails(
    meshing: &mut Meshing,
    pal: &Palette,
    route: &Route,
    lift: f32,
) {
    let total = route.total();
    let frame = |s: f32| {
        let (p, tan) = route.at(s);
        (p, tan, Vec3::new(-tan.z, 0.0, tan.x))
    };
    let on_plaza = |s: f32| {
        let i = route.line.len.partition_point(|&l| l < s);
        route.on_plaza[i.min(route.on_plaza.len() - 1)]
    };
    let count = (total / 0.62).floor() as usize;
    for k in 0..count {
        let s = (k as f32 + 0.5) * total / count as f32;
        if on_plaza(s) {
            continue;
        }
        let (p, tan, across) = frame(s);
        let tone = 0.85 + 0.3 * unit(key(&[k as u32, 65]));
        let col = pal.timber_dark * tone;
        let (hw, hl, y0, y1) =
            (0.58, 0.085, lift - 0.01, lift + 0.05);
        let corner = |a: f32, l: f32, y: f32| {
            p + across * a * hw + tan * l * hl + Vec3::Y * y
        };
        meshing.face(
            [
                corner(-1.0, -1.0, y1),
                corner(1.0, -1.0, y1),
                corner(1.0, 1.0, y1),
                corner(-1.0, 1.0, y1),
            ],
            Vec3::Y,
            col,
        );
        for l in [-1.0f32, 1.0] {
            meshing.face(
                [
                    corner(-1.0, l, y0),
                    corner(1.0, l, y0),
                    corner(1.0, l, y1),
                    corner(-1.0, l, y1),
                ],
                tan * l,
                col * 0.7,
            );
        }
    }
    let (steel, bright) = (pal.steel, pal.steel_bright);
    let steps = (total / 1.0).ceil() as usize;
    for side in [-1.0f32, 1.0] {
        let mut prev: Option<[Vec3; 4]> = None;
        for k in 0..=steps {
            let s = total * k as f32 / steps as f32;
            if on_plaza(s) {
                prev = None;
                continue;
            }
            let (p, _, across) = frame(s);
            let foot = p + across * side * 0.3;
            let (y0, y1) = (lift + 0.04, lift + 0.11);
            let ring = [
                foot - across * 0.03 + Vec3::Y * y0,
                foot - across * 0.03 + Vec3::Y * y1,
                foot + across * 0.03 + Vec3::Y * y1,
                foot + across * 0.03 + Vec3::Y * y0,
            ];
            if let Some(q) = prev {
                meshing.face(
                    [q[1], q[2], ring[2], ring[1]],
                    Vec3::Y,
                    bright,
                );
                meshing.face(
                    [q[0], q[1], ring[1], ring[0]],
                    -across,
                    steel,
                );
                meshing.face(
                    [q[3], q[2], ring[2], ring[3]],
                    across,
                    steel,
                );
            }
            prev = Some(ring);
        }
    }
}

/// Stone decks, walls and parapets wherever the route stands clear of
/// the ground, with arches over the water. A parapet stops where it
/// would stand on the deck of a bridge of `met`.
fn bridges(
    meshing: &mut Meshing,
    land: &Land,
    pal: &Palette,
    route: &Route,
    lift: f32,
    met: &[Span],
) {
    let (water, half) =
        (land.set.land.water_y, land.set.road.bridge_half);
    if !route.raised.iter().any(|&r| r > land.set.road.bridge_from) {
        return;
    }
    let all = rows(land, route, 1);
    flags(meshing, pal, &all, half - 0.16, lift, true);
    let under = |row: &Row| {
        let ground = row.c.y - route.raised[row.i];
        if ground > water + 0.3 {
            return ground - 0.3;
        }
        // Arches a little over three units apart, with piers
        // between.
        let x = (row.s + 1.6).rem_euclid(3.2) - 1.6;
        let r = 1.25f32;
        if x.abs() < r {
            (water - 0.05 + (r * r - x * x).sqrt() * 0.8)
                .min(row.c.y - 0.16)
        } else {
            -1.6
        }
    };
    for w in all.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        if !(a.bridge && b.bridge) {
            continue;
        }
        let cell = key(&[a.i as u32, 66]);
        let stone = pal
            .limestone
            .lerp(pal.limestone_shade, 0.2 + 0.35 * unit(cell));
        // The bed the flags lie on.
        meshing.face(
            [
                a.at(-half, lift),
                a.at(half, lift),
                b.at(half, lift),
                b.at(-half, lift),
            ],
            Vec3::Y,
            pal.limestone_shade * 0.8,
        );
        for side in [-1.0f32, 1.0] {
            let out = a.across * side;
            let (wa, wb) = (side * half, side * half);
            let foot = |row: &Row, w: f32| {
                let mut p = row.at(w, 0.0);
                p.y = under(row);
                p
            };
            meshing.face_cols(
                [
                    foot(a, wa),
                    foot(b, wb),
                    b.at(wb, lift),
                    a.at(wa, lift),
                ],
                out,
                [stone * 0.6, stone * 0.6, stone, stone],
            );
            let block = a.c.lerp(b.c, 0.5) + out * (half - 0.085);
            if met.iter().any(|span| span.is_on_deck(block, half)) {
                continue;
            }
            // The parapet: one block to each step, some taller.
            let tall =
                0.2 + 0.07 * unit(key(&[cell, side as i32 as u32]));
            let block = pal.limestone
                * (0.92
                    + 0.14
                        * unit(key(&[cell, 2, side as i32 as u32])));
            let (inner, outer) = (wa - side * 0.17, wa);
            let (p, q) = (a.c.lerp(b.c, 0.04), b.c.lerp(a.c, 0.04));
            let at = |m: Vec3, w: f32, h: f32| {
                m + a.across * w + Vec3::Y * (lift + h)
            };
            meshing.face(
                [
                    at(p, inner, tall),
                    at(p, outer, tall),
                    at(q, outer, tall),
                    at(q, inner, tall),
                ],
                Vec3::Y,
                block,
            );
            meshing.face(
                [
                    at(p, inner, 0.0),
                    at(q, inner, 0.0),
                    at(q, inner, tall),
                    at(p, inner, tall),
                ],
                -out,
                block * 0.8,
            );
            meshing.face(
                [
                    at(p, outer, 0.0),
                    at(q, outer, 0.0),
                    at(q, outer, tall),
                    at(p, outer, tall),
                ],
                out,
                block * 0.9,
            );
        }
    }
}

/// Height of the groove above the route's surface.
pub(crate) fn groove_lift(level: RoadLevel, salt: u32) -> f32 {
    lift(level, salt)
        + match level {
            RoadLevel::Rail => 0.06,
            _ => 0.034,
        }
}

/// The groove: a slate bed with the light strip down its middle. UV.x
/// is the distance along the road, as the inlay shader expects.
pub fn groove(
    land: &Land,
    route: &Route,
    level: RoadLevel,
    salt: u32,
) -> Mesh {
    ribbon(
        land,
        route,
        level,
        land.set.road.groove_width,
        groove_lift(level, salt),
        0.07,
    )
}

fn ribbon(
    land: &Land,
    route: &Route,
    level: RoadLevel,
    width: f32,
    lift: f32,
    skirt: f32,
) -> Mesh {
    let mut meshing = Meshing::new();
    let overgrown = level == RoadLevel::Overgrown;
    // An unused groove is whole too, and dull with dust.
    let tint = if overgrown {
        Vec3::new(0.62, 0.56, 0.46)
    } else {
        Vec3::ONE
    };
    let mut prev: Option<[u32; 4]> = None;
    for row in rows(land, route, 2) {
        // On a common the groove hands over to the common's own ring.
        if row.plaza {
            prev = None;
            continue;
        }
        let n = row.across;
        let (l, r) = (
            row.c - n * width * 0.5 + Vec3::Y * lift,
            row.c + n * width * 0.5 + Vec3::Y * lift,
        );
        let ids = [
            meshing.vuv(
                l - Vec3::Y * skirt,
                -n,
                tint * 0.6,
                Vec2::new(row.s, -1.0),
            ),
            meshing.vuv(l, Vec3::Y, tint, Vec2::new(row.s, -1.0)),
            meshing.vuv(r, Vec3::Y, tint, Vec2::new(row.s, 1.0)),
            meshing.vuv(
                r - Vec3::Y * skirt,
                n,
                tint * 0.6,
                Vec2::new(row.s, 1.0),
            ),
        ];
        if let Some(q) = prev {
            meshing.quad(q[1], q[2], ids[2], ids[1]);
            if skirt > 0.0 {
                meshing.quad(q[0], q[1], ids[1], ids[0]);
                meshing.quad(q[2], q[3], ids[3], ids[2]);
            }
        }
        prev = Some(ids);
    }
    meshing.build()
}
