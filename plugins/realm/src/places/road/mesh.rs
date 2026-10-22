//! Road meshes: a ribbon laid on the ground along a route, at one of
//! four levels of use, with stone bridges where it crosses water and
//! the groove for its light.

use std::ops::RangeInclusive;

use glam::{Vec2, Vec3, Vec3Swizzles};

use crate::build::mesh::{Mb, smoothstep};
use crate::society::RoadLevel;
use crate::world::land::Land;
use crate::world::layout::Route;
use crate::world::noise::{key, perlin, unit};
use crate::world::palette::Palette;

/// Height of a level's surface above the ground. Busier roads lie on
/// top where two share a stretch.
fn lift(level: RoadLevel, salt: u64) -> f32 {
    let base = match level {
        RoadLevel::Overgrown => 0.05,
        RoadLevel::Dirt => 0.06,
        RoadLevel::Paved => 0.078,
        RoadLevel::Rail => 0.094,
    };
    base + 0.008 * unit(key(&[salt, 61]))
}

/// The rows a level of use lays its surface on: every third point,
/// or every point of a paved road.
fn stride(level: RoadLevel) -> usize {
    match level {
        RoadLevel::Paved => 1,
        _ => 3,
    }
}

/// The rows the groove is laid on.
const GROOVE_STRIDE: usize = 2;

/// One cross-section of a road.
pub(super) struct Row {
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

/// True where the road is flat: on a bridge or on a common.
fn flat(land: &Land, route: &Route, i: usize) -> bool {
    route.raised[i] > land.set.road.bridge_from || route.on_plaza[i]
}

/// True where the road's point `i` is one a row of `stride` falls
/// on: one of every `stride`, the last, those on flat ground and
/// those beside a change of it.
fn kept(land: &Land, route: &Route, i: usize, stride: usize) -> bool {
    let n = route.line.pts.len();
    let edge = i + 1 == n
        || (i > 0
            && flat(land, route, i) != flat(land, route, i - 1))
        || (i + 1 < n
            && flat(land, route, i) != flat(land, route, i + 1));
    i.is_multiple_of(stride) || edge || flat(land, route, i)
}

/// The cross-section at point `i` of `route`.
fn row(land: &Land, route: &Route, i: usize) -> Row {
    let n = route.line.pts.len();
    let p = route.line.pts[i];
    let (a, b) = (
        route.line.pts[i.saturating_sub(1)],
        route.line.pts[(i + 1).min(n - 1)],
    );
    let tan = (b - a).normalize_or_zero();
    let across = Vec3::new(-tan.z, 0.0, tan.x);
    let c = Vec3::new(p.x, route.y[i], p.z);
    let tilt = if flat(land, route, i) {
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
    let over =
        (across * 2.0 + Vec3::Y * (tilt[1] - tilt[0])).normalize();
    let up = over.cross(along).normalize();
    let bridge_from = land.set.road.bridge_from;
    Row {
        i,
        s: route.line.len[i],
        c,
        across,
        up: if up.y < 0.0 { -up } else { up },
        tilt,
        bridge: route.raised[i] > bridge_from && !route.on_plaza[i],
        plaza: route.on_plaza[i],
    }
}

/// The rows of a route that a road of a level of use is laid on,
/// worked out a few at a time, since each looks up the ground.
pub(super) struct Rows {
    /// The points a row is wanted at, in order.
    wanted: Vec<usize>,
    rows: Vec<Row>,
}

impl Rows {
    /// The rows a road of `level` along `route` needs: those of its
    /// surface, of its groove, and every one where it bridges.
    pub(super) fn new(
        land: &Land,
        route: &Route,
        level: RoadLevel,
    ) -> Self {
        let bridged = route
            .raised
            .iter()
            .any(|&r| r > land.set.road.bridge_from);
        let mut strides = vec![stride(level), GROOVE_STRIDE];
        if bridged {
            strides.push(1);
        }
        let wanted = (0..route.line.pts.len())
            .filter(|&i| {
                strides.iter().any(|&s| kept(land, route, i, s))
            })
            .collect();
        Self {
            wanted,
            rows: Vec::new(),
        }
    }

    /// Works out up to `most` more rows. True once all are.
    pub(super) fn fill(
        &mut self,
        land: &Land,
        route: &Route,
        most: usize,
    ) -> bool {
        let from = self.rows.len();
        let to = (from + most).min(self.wanted.len());
        for &i in &self.wanted[from..to] {
            self.rows.push(row(land, route, i));
        }
        self.rows.len() == self.wanted.len()
    }

    /// The stretches of `most` rows each, from the first to the last,
    /// that the road's meshes are built in.
    pub(super) fn stretches(&self, most: usize) -> Vec<Stretch> {
        let most = most.max(1);
        let count = self.rows.len().div_ceil(most).max(1);
        (0..count)
            .map(|k| Stretch {
                from: match k {
                    0 => f32::NEG_INFINITY,
                    _ => self.rows[k * most].s,
                },
                to: self
                    .rows
                    .get((k + 1) * most)
                    .map_or(f32::INFINITY, |row| row.s),
            })
            .collect()
    }

    /// The rows of `stride`.
    fn of(
        &self,
        land: &Land,
        route: &Route,
        stride: usize,
    ) -> Vec<&Row> {
        self.rows
            .iter()
            .filter(|row| kept(land, route, row.i, stride))
            .collect()
    }
}

/// A stretch of a road, by distance along it: its rows from the first
/// at `from` or past it to the first at `to` or past it, so that two
/// stretches that meet share a row.
#[derive(Clone, Copy, Debug)]
pub(super) struct Stretch {
    from: f32,
    to: f32,
}

impl Stretch {
    /// The rows of `rows` within it.
    fn cut<'a>(self, rows: &[&'a Row]) -> Vec<&'a Row> {
        let first = rows.partition_point(|row| row.s < self.from);
        let last = rows.partition_point(|row| row.s < self.to);
        rows[first.min(rows.len())..(last + 1).min(rows.len())]
            .to_vec()
    }

    /// The steps of `steps` along a road `total` long within it,
    /// those at either end included.
    fn steps(
        self,
        total: f32,
        steps: usize,
    ) -> RangeInclusive<usize> {
        let step = |s: f32| {
            (s / total.max(1e-6) * steps as f32)
                .floor()
                .clamp(0.0, steps as f32) as usize
        };
        step(self.from)..=step(self.to)
    }

    fn holds(self, s: f32) -> bool {
        self.from <= s && s < self.to
    }
}

/// The ground a span lies over, from its least corner to its most.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Over {
    lo: Vec2,
    hi: Vec2,
}

impl Over {
    fn with(self, p: Vec2) -> Self {
        Self {
            lo: self.lo.min(p),
            hi: self.hi.max(p),
        }
    }

    fn inflate(self, by: f32) -> Self {
        Self {
            lo: self.lo - by,
            hi: self.hi + by,
        }
    }

    fn meets(self, other: Self) -> bool {
        self.lo.cmple(other.hi).all() && other.lo.cmple(self.hi).all()
    }

    fn contains(self, p: Vec2) -> bool {
        self.lo.cmple(p).all() && p.cmple(self.hi).all()
    }
}

/// The stretches of a road that are bridge, for the roads that meet
/// it there.
#[derive(Clone, PartialEq, Debug)]
pub struct Span {
    /// The middle of the deck, a step at a time.
    steps: Vec<[Vec3; 2]>,
    over: Over,
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
        let over = steps.iter().flatten().fold(
            Over {
                lo: first,
                hi: first,
            },
            |over, p| over.with(p.xz()),
        );
        Some(Span { steps, over })
    }

    /// True when the two come within `reach` of each other, by the
    /// ground each lies over.
    pub(super) fn nears(&self, other: &Span, reach: f32) -> bool {
        self.over.inflate(reach).meets(other.over)
    }

    /// A number that is another for another span.
    pub(super) fn mark(&self) -> u64 {
        let bits = |v: f32| u64::from(v.to_bits());
        let (first, last) =
            (self.steps[0][0], self.steps[self.steps.len() - 1][1]);
        key(&[
            self.steps.len() as u64,
            bits(first.x),
            bits(first.z),
            bits(last.x),
            bits(last.z),
        ])
    }

    /// True when `p` is on its deck, which is `half` wide to each
    /// side. A road that passes over or under it is not on it.
    fn holds(&self, p: Vec3, half: f32) -> bool {
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

/// The road's surface for a level of use along `stretch`. `salt` is
/// the road's own number, which settles its small differences. Where
/// a bridge of `met` crosses a bridge of its own, the two are one
/// deck.
pub(super) fn surface(
    land: &Land,
    route: &Route,
    rows: &Rows,
    level: RoadLevel,
    salt: u64,
    met: &[Span],
    stretch: Stretch,
) -> Mb {
    let pal = &land.pal;
    let mut mb = Mb::new();
    let lift = lift(level, salt);
    let laid = stretch.cut(&rows.of(land, route, stride(level)));
    shoulder(&mut mb, land, pal, &laid, level, lift);
    match level {
        RoadLevel::Paved => {
            flags(
                &mut mb,
                pal,
                &laid,
                land.set.road.flags_half,
                lift,
                false,
            );
        }
        RoadLevel::Rail => rails(&mut mb, pal, route, lift, stretch),
        _ => {}
    }
    bridges(&mut mb, land, pal, route, rows, lift, met, stretch);
    mb
}

/// The trodden strip under every road: soil in the middle that fades
/// into the grass at both edges.
fn shoulder(
    mb: &mut Mb,
    land: &Land,
    pal: &Palette,
    rows: &[&Row],
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
            mb.v(row.at(w, h), row.up, col)
        });
        if let Some(q) = prev {
            for k in 0..4 {
                mb.quad(q[k], q[k + 1], ids[k + 1], ids[k]);
            }
        }
        prev = Some(ids);
    }
}

/// Stone flags, three across, each its own block.
fn flags(
    mb: &mut Mb,
    pal: &Palette,
    rows: &[&Row],
    half: f32,
    lift: f32,
    on_bridge: bool,
) {
    let h = lift + 0.012;
    let bands = [(-half, -0.21), (-0.21, 0.21), (0.21, half)];
    for w in rows.windows(2) {
        let (a, b) = (w[0], w[1]);
        if a.plaza || b.plaza || (a.bridge && b.bridge) != on_bridge {
            continue;
        }
        if !on_bridge && (a.bridge || b.bridge) {
            continue;
        }
        for (k, &(w0, w1)) in bands.iter().enumerate() {
            // Coloured by place, so two roads on one stretch agree.
            let cell = key(&[
                (a.c.x * 2.0).round() as i64 as u64,
                (a.c.z * 2.0).round() as i64 as u64,
                k as u64,
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
            mb.face(
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

/// Sleepers on the ballast and two rails over them, along `stretch`.
fn rails(
    mb: &mut Mb,
    pal: &Palette,
    route: &Route,
    lift: f32,
    stretch: Stretch,
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
        if !stretch.holds(s) || on_plaza(s) {
            continue;
        }
        let (p, tan, across) = frame(s);
        let tone = 0.85 + 0.3 * unit(key(&[k as u64, 65]));
        let col = pal.timber_dark * tone;
        let (hw, hl, y0, y1) =
            (0.58, 0.085, lift - 0.01, lift + 0.05);
        let corner = |a: f32, l: f32, y: f32| {
            p + across * a * hw + tan * l * hl + Vec3::Y * y
        };
        mb.face(
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
            mb.face(
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
    let steps = (total.ceil() as usize).max(1);
    for side in [-1.0f32, 1.0] {
        let mut prev: Option<[Vec3; 4]> = None;
        for k in stretch.steps(total, steps) {
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
                mb.face(
                    [q[1], q[2], ring[2], ring[1]],
                    Vec3::Y,
                    bright,
                );
                mb.face(
                    [q[0], q[1], ring[1], ring[0]],
                    -across,
                    steel,
                );
                mb.face(
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
/// the ground along `stretch`, with arches over the water. A parapet
/// stops where it would stand on the deck of a bridge of `met`.
fn bridges(
    mb: &mut Mb,
    land: &Land,
    pal: &Palette,
    route: &Route,
    rows: &Rows,
    lift: f32,
    met: &[Span],
    stretch: Stretch,
) {
    let (water, half) =
        (land.set.land.water_y, land.set.road.bridge_half);
    if !route.raised.iter().any(|&r| r > land.set.road.bridge_from) {
        return;
    }
    let all = stretch.cut(&rows.of(land, route, 1));
    flags(mb, pal, &all, half - 0.16, lift, true);
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
        let (a, b) = (w[0], w[1]);
        if !(a.bridge && b.bridge) {
            continue;
        }
        let cell = key(&[a.i as u64, 66]);
        let stone = pal
            .limestone
            .lerp(pal.limestone_shade, 0.2 + 0.35 * unit(cell));
        // The bed the flags lie on.
        mb.face(
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
            let edge = side * half;
            let foot = |row: &Row, w: f32| {
                let mut p = row.at(w, 0.0);
                p.y = under(row);
                p
            };
            mb.face_cols(
                [
                    foot(a, edge),
                    foot(b, edge),
                    b.at(edge, lift),
                    a.at(edge, lift),
                ],
                out,
                [stone * 0.6, stone * 0.6, stone, stone],
            );
            let block = a.c.lerp(b.c, 0.5) + out * (half - 0.085);
            if met.iter().any(|span| span.holds(block, half)) {
                continue;
            }
            // The parapet: one block to each step, some taller.
            let tall =
                0.2 + 0.07 * unit(key(&[cell, side as i64 as u64]));
            let block = pal.limestone
                * (0.92
                    + 0.14
                        * unit(key(&[cell, 2, side as i64 as u64])));
            let (inner, outer) = (edge - side * 0.17, edge);
            let (p, q) = (a.c.lerp(b.c, 0.04), b.c.lerp(a.c, 0.04));
            let at = |m: Vec3, w: f32, h: f32| {
                m + a.across * w + Vec3::Y * (lift + h)
            };
            mb.face(
                [
                    at(p, inner, tall),
                    at(p, outer, tall),
                    at(q, outer, tall),
                    at(q, inner, tall),
                ],
                Vec3::Y,
                block,
            );
            mb.face(
                [
                    at(p, inner, 0.0),
                    at(q, inner, 0.0),
                    at(q, inner, tall),
                    at(p, inner, tall),
                ],
                -out,
                block * 0.8,
            );
            mb.face(
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
pub(super) fn groove_lift(level: RoadLevel, salt: u64) -> f32 {
    lift(level, salt)
        + match level {
            RoadLevel::Rail => 0.06,
            _ => 0.034,
        }
}

/// The groove along `stretch`: a slate bed with the light strip down
/// its middle. UV.x is the distance along the road and UV.y runs from
/// -1 to 1 across it, as the groove's shader expects.
pub(super) fn groove(
    land: &Land,
    route: &Route,
    rows: &Rows,
    level: RoadLevel,
    salt: u64,
    stretch: Stretch,
) -> Mb {
    let width = land.set.road.groove_width;
    let (lift, skirt) = (groove_lift(level, salt), 0.07);
    let mut mb = Mb::new();
    // An unused groove is whole too, and dull with dust.
    let tint = if level == RoadLevel::Overgrown {
        Vec3::new(0.62, 0.56, 0.46)
    } else {
        Vec3::ONE
    };
    let mut prev: Option<[u32; 4]> = None;
    for row in stretch.cut(&rows.of(land, route, GROOVE_STRIDE)) {
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
            mb.vuv(
                l - Vec3::Y * skirt,
                -n,
                tint * 0.6,
                Vec2::new(row.s, -1.0),
            ),
            mb.vuv(l, Vec3::Y, tint, Vec2::new(row.s, -1.0)),
            mb.vuv(r, Vec3::Y, tint, Vec2::new(row.s, 1.0)),
            mb.vuv(
                r - Vec3::Y * skirt,
                n,
                tint * 0.6,
                Vec2::new(row.s, 1.0),
            ),
        ];
        if let Some(q) = prev {
            mb.quad(q[1], q[2], ids[2], ids[1]);
            mb.quad(q[0], q[1], ids[1], ids[0]);
            mb.quad(q[2], q[3], ids[3], ids[2]);
        }
        prev = Some(ids);
    }
    mb
}
