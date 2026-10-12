//! The static scene: meadow, canal, bridge, stall, signal post, a
//! cottage for the backdrop, and planting. Everything is generated
//! from fixed seeds.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bevy::prelude::*;
use hamlet_assets::tint::hex;
use hamlet_world::meshes::{Line, Mb, Rng, fbm, noise2, smoothstep};
use hamlet_world::palette::Palette;

use super::cottage::cottage;
use super::planting::{fence, planting};
use super::stall::{signal_post, stall};

/// The places of the things in the scene, and the sizes more than one
/// of them goes by.
#[derive(Clone)]
pub struct Places {
    pub water_y: f32,
    pub step_top: f32,
    pub stall: Vec3,
    pub doorstep: Vec3,
    pub doorstep_half: Vec2,
    pub signal_post: Vec3,
    pub cottage: Vec3,
    pub bridge_half: f32,
    pub bridge_width: f32,
    pub arch_r: f32,
    /// Heights of the signal post's arm and lamp.
    pub signal_arm_y: f32,
    pub signal_lamp_y: f32,
    /// The middle of the row of robots, one per state.
    pub lineup: Vec3,
    /// Width of the groove's slate bed, and of the ribbon that
    /// carries its night wash.
    pub groove_width: f32,
    pub wash_width: f32,
    /// Speed of a mote along the groove, in units per second.
    pub mote_speed: f32,
    /// The start of the visible part of the path.
    pub visible_from: f32,
    /// The f-stop of the blur in the far views.
    pub blur_stop: f32,
}

impl Default for Places {
    fn default() -> Self {
        Self {
            water_y: -0.42,
            step_top: 0.09,
            stall: Vec3::new(3.6, 0.0, -3.6),
            doorstep: Vec3::new(3.6, 0.0, -2.0),
            doorstep_half: Vec2::new(0.85, 0.5),
            signal_post: Vec3::new(5.75, 0.0, -1.7),
            cottage: Vec3::new(-0.6, 0.0, -5.9),
            bridge_half: 1.5,
            bridge_width: 1.5,
            arch_r: 0.76,
            signal_arm_y: 1.98,
            signal_lamp_y: 2.44,
            lineup: Vec3::new(1.3, 0.0, 4.6),
            groove_width: 0.2,
            wash_width: 0.9,
            mote_speed: 3.0,
            visible_from: 2.0,
            blur_stop: 0.12,
        }
    }
}

impl Places {
    /// The place the stall's lamp hangs at.
    pub fn stall_lamp(&self) -> Vec3 {
        Vec3::new(self.stall.x + 1.38, 1.52, self.stall.z + 1.32)
    }

    /// The point on the signal post a tag's string is tied to.
    pub fn knot(&self) -> Vec3 {
        Vec3::new(self.signal_post.x, 1.62, self.signal_post.z + 0.07)
    }
}

/// The lie of the land: the path, the canal and the heights they
/// imply.
#[derive(Resource)]
pub struct Terrain {
    pub path: Line,
    pub canal: Line,
    pub(super) bridge_c: Vec3,
    bridge_t: Vec3,
    pub at: Places,
}

impl Default for Terrain {
    fn default() -> Self {
        Self::new()
    }
}

impl Terrain {
    pub fn new() -> Self {
        let at = Places::default();
        let c = Vec2::new(-3.0, 2.5);
        let t = Vec2::new(1.0, -0.12).normalize();
        let n = Vec2::new(-t.y, t.x);
        let path = Line::spline(
            &[
                Vec2::new(-12.5, 5.6),
                Vec2::new(-10.0, 4.5),
                Vec2::new(-7.6, 3.5),
                c - t * 2.3,
                c,
                c + t * 2.3,
                Vec2::new(1.4, 1.25),
                Vec2::new(2.95, -0.1),
                Vec2::new(3.5, -1.2),
                Vec2::new(at.doorstep.x, -1.7),
                Vec2::new(at.doorstep.x, -2.34),
            ],
            0.05,
        );
        let canal = Line::spline(
            &[
                Vec2::new(-9.5, -22.0),
                Vec2::new(-7.2, -13.0),
                Vec2::new(-4.9, -5.0),
                c - n * 2.6,
                c,
                c + n * 2.6,
                Vec2::new(-1.3, 9.5),
                Vec2::new(1.2, 16.0),
                Vec2::new(4.0, 24.0),
            ],
            0.1,
        );
        Self {
            path,
            canal,
            bridge_c: Vec3::new(c.x, 0.0, c.y),
            bridge_t: Vec3::new(t.x, 0.0, t.y),
            at,
        }
    }

    pub fn ground_y(&self, x: f32, z: f32) -> f32 {
        let far = smoothstep(
            7.0,
            20.0,
            Vec2::new(x + 1.0, z + 1.0).length(),
        );
        let hills = (fbm(x * 0.07 + 3.0, z * 0.07 - 1.0) - 0.45)
            * (0.12 + 2.6 * far);
        let lumps = (noise2(x * 0.6, z * 0.6) - 0.5) * 0.05;
        let (d, _) = self.canal.nearest(x, z);
        let cut = smoothstep(0.94, 0.76, d);
        (hills + lumps) * (1.0 - cut) - cut * 0.95
    }

    fn deck_y(&self, u: f32) -> f32 {
        let k = (u / self.at.bridge_half).clamp(-1.0, 1.0);
        0.1 + 0.46 * (1.0 - k * k)
    }

    /// The height of the path's surface at arc length `s`.
    pub fn path_y(&self, s: f32) -> f32 {
        let (p, _) = self.path.at(s);
        let u = (p - self.bridge_c).dot(self.bridge_t);
        let ground = self.ground_y(p.x, p.z);
        if (p.x - self.at.doorstep.x).abs() < self.at.doorstep_half.x
            && (p.z - self.at.doorstep.z).abs()
                < self.at.doorstep_half.y
        {
            return self.at.step_top;
        }
        if u.abs() < self.at.bridge_half + 0.3 {
            let ramp = smoothstep(
                self.at.bridge_half + 0.3,
                self.at.bridge_half - 0.1,
                u.abs(),
            );
            return ground.max(0.0) * (1.0 - ramp)
                + self.deck_y(u) * ramp;
        }
        ground
    }

    /// A point on the path's surface, and the direction of travel.
    pub fn on_path(&self, s: f32) -> (Vec3, Vec3) {
        let (p, t) = self.path.at(s);
        (Vec3::new(p.x, self.path_y(s), p.z), t)
    }

    /// 1 on the trodden path, 0 on grass.
    pub(super) fn dirt(&self, x: f32, z: f32) -> f32 {
        let (d, s) = self.path.nearest(x, z);
        let half = 0.6 * (1.0 + 0.3 * (noise2(s * 0.9, 4.0) - 0.5))
            + 0.1 * (noise2(x * 2.2, z * 2.2) - 0.5);
        let trodden = Vec2::new(
            x - self.at.doorstep.x,
            z - self.at.doorstep.z + 0.1,
        )
        .length();
        let yard = smoothstep(
            1.9,
            1.1,
            trodden + 0.5 * (noise2(x * 1.7, z * 1.7) - 0.5),
        );
        smoothstep(half + 0.2, half - 0.14, d).max(yard)
    }

    pub(super) fn grass_colour(
        &self,
        pal: &Palette,
        x: f32,
        z: f32,
    ) -> Vec3 {
        let broad = fbm(x * 0.16 + 9.0, z * 0.16);
        let fine = noise2(x * 1.3, z * 1.3);
        let base = pal
            .grass_shade
            .lerp(pal.grass, smoothstep(0.32, 0.62, broad));
        base.lerp(
            pal.grass_sun,
            0.45 * fine * smoothstep(0.35, 0.7, broad),
        )
    }

    fn ground_colour(
        &self,
        pal: &Palette,
        x: f32,
        z: f32,
        y: f32,
    ) -> Vec3 {
        let grass = self.grass_colour(pal, x, z);
        let dirt = self.dirt(x, z);
        let (d, s) = self.path.nearest(x, z);
        // Two faint ruts either side of the groove, and a paler
        // crown.
        let rut = smoothstep(0.12, 0.0, (d - 0.3).abs()) * 0.5;
        let grain = noise2(x * 3.1 + s, z * 3.1);
        let soil = pal.dirt.lerp(
            pal.dirt_shade,
            (0.25 + 0.5 * grain + rut).clamp(0.0, 1.0),
        );
        let mut c = grass.lerp(soil, dirt);
        let wet =
            smoothstep(self.at.water_y + 0.5, self.at.water_y, y);
        c = c.lerp(hex("#5E5B45"), wet);
        c
    }
}

/// Meshes grouped by the material they need.
#[derive(Default)]
pub struct Built {
    pub ground: Mb,
    /// Stone, timber, plaster: single-sided, casts shadows.
    pub solid: Mb,
    /// Canvas: double-sided, casts shadows.
    pub cloth: Mb,
    /// Tree crowns and bushes: unculled, lit from one side, casts
    /// shadows.
    pub leaf: Mb,
    /// Grass and flowers: unculled, lit from one side, casts
    /// nothing.
    pub tuft: Mb,
    pub water: Mb,
    pub brass: Mb,
    pub iron: Mb,
    pub window: Mb,
}

pub fn build(terrain: &Terrain, pal: &Palette) -> Built {
    let mut b = Built::default();
    ground(&mut b.ground, terrain, pal);
    water(&mut b, terrain, pal);
    quay(&mut b.solid, terrain, pal);
    bridge(&mut b.solid, terrain, pal);
    doorstep(&mut b.solid, terrain, pal);
    stall(&mut b, terrain, pal);
    signal_post(&mut b, terrain, pal);
    cottage(&mut b, terrain, pal);
    fence(&mut b.solid, terrain, pal);
    planting(&mut b, terrain, pal);
    b
}

fn ground(mb: &mut Mb, t: &Terrain, pal: &Palette) {
    // Fine cells where the camera looks, stretching toward the
    // horizon.
    let axis = |lo: f32, hi: f32| {
        let mut v = Vec::new();
        let mut x = lo;
        while x <= hi + 1e-3 {
            v.push(x);
            x += 0.16;
        }
        let mut step = 0.2;
        let (mut a, mut c) = (lo, hi);
        while c < 70.0 {
            step *= 1.22;
            a -= step;
            c += step;
            v.insert(0, a);
            v.push(c);
        }
        v
    };
    let (xs, zs) = (axis(-14.0, 11.0), axis(-12.0, 9.5));
    let mut ids = vec![vec![0u32; zs.len()]; xs.len()];
    for (i, &x) in xs.iter().enumerate() {
        for (j, &z) in zs.iter().enumerate() {
            let y = t.ground_y(x, z);
            let e = 0.08;
            let n = Vec3::new(
                t.ground_y(x - e, z) - t.ground_y(x + e, z),
                2.0 * e,
                t.ground_y(x, z - e) - t.ground_y(x, z + e),
            );
            ids[i][j] = mb.v(
                Vec3::new(x, y, z),
                n.normalize(),
                t.ground_colour(pal, x, z, y),
            );
        }
    }
    for i in 0..xs.len() - 1 {
        for j in 0..zs.len() - 1 {
            mb.quad(
                ids[i][j],
                ids[i][j + 1],
                ids[i + 1][j + 1],
                ids[i + 1][j],
            );
        }
    }
}

fn water(b: &mut Built, t: &Terrain, pal: &Palette) {
    let total = t.canal.total();
    let (deep, edge) = (
        pal.water_shade * 0.8,
        pal.water_shade.lerp(pal.water, 0.35),
    );
    let mut prev: Option<[u32; 3]> = None;
    let mut s = 0.0;
    while s <= total {
        let (p, tan) = t.canal.at(s);
        let n = Vec3::new(tan.z, 0.0, -tan.x);
        let y = Vec3::Y * t.at.water_y;
        let row = [
            b.water.v(p - n * 0.98 + y, Vec3::Y, edge),
            b.water.v(p + y, Vec3::Y, deep),
            b.water.v(p + n * 0.98 + y, Vec3::Y, edge),
        ];
        if let Some(q) = prev {
            b.water.quad(q[0], q[1], row[1], row[0]);
            b.water.quad(q[1], q[2], row[2], row[1]);
        }
        prev = Some(row);
        s += 0.4;
    }
    // Lily pads: a few flat leaves, a notch cut from each.
    let mut rng = Rng::seeded("lily pads");
    let pad = hex("#6F9A58");
    for _ in 0..26 {
        let s = rng.range(total * 0.3, total * 0.62);
        let (p, tan) = t.canal.at(s);
        let n = Vec3::new(tan.z, 0.0, -tan.x);
        let c =
            p + n * rng.sym(0.55) + Vec3::Y * (t.at.water_y + 0.012);
        if (c - t.bridge_c).length() < 1.2 {
            continue;
        }
        let r = rng.range(0.09, 0.17);
        let col = pad * rng.value(0.15);
        let start = rng.f() * TAU;
        let m = b.tuft.v(c, Vec3::Y, col * 0.85);
        let rim: Vec<u32> = (0..9)
            .map(|i| {
                let a = start + i as f32 / 9.0 * TAU * 0.9;
                b.tuft.v(
                    c + Vec3::new(a.cos(), 0.0, a.sin()) * r,
                    Vec3::Y,
                    col,
                )
            })
            .collect();
        for w in rim.windows(2) {
            b.tuft.tri(m, w[0], w[1]);
        }
    }
}

/// Dressed stone along both banks, each block a little out of true.
fn quay(mb: &mut Mb, t: &Terrain, pal: &Palette) {
    let mut rng = Rng::seeded("canal quay");
    for side in [-1.0f32, 1.0] {
        let mut s = t.canal.total() * 0.18;
        while s < t.canal.total() * 0.8 {
            let len = rng.range(0.42, 0.7);
            let (p, tan) = t.canal.at(s + len * 0.5);
            let n = Vec3::new(tan.z, 0.0, -tan.x);
            let c = p + n * side * (0.92 + rng.sym(0.025));
            let under_bridge = (c - t.bridge_c).dot(t.bridge_t).abs()
                < t.at.bridge_half
                && (c - t.bridge_c).length() < 1.7;
            if !under_bridge {
                let yaw = Quat::from_rotation_y(tan.x.atan2(tan.z))
                    * rng.wobble(2.0);
                let col = pal
                    .limestone
                    .lerp(pal.limestone_shade, rng.f() * 0.35)
                    * rng.value(0.05);
                let h = rng.range(0.52, 0.58);
                mb.rbox(
                    c + Vec3::Y * (0.1 - h),
                    Vec3::new(0.19, h, len * 0.5 - 0.012),
                    0.04,
                    yaw,
                    col,
                    0.45,
                );
            }
            s += len;
        }
    }
}

fn bridge(mb: &mut Mb, t: &Terrain, pal: &Palette) {
    let mut rng = Rng::seeded("bridge by the stall");
    let (c, tan) = (t.bridge_c, t.bridge_t);
    let n = Vec3::new(-tan.z, 0.0, tan.x);
    let hw = t.at.bridge_width * 0.5;
    let under = |u: f32| {
        if u.abs() < t.at.arch_r {
            t.at.water_y - 0.05
                + (t.at.arch_r * t.at.arch_r - u * u).sqrt() * 0.92
        } else {
            -0.7
        }
    };
    let at =
        |u: f32, w: f32, y: f32| c + tan * u + n * w + Vec3::Y * y;
    // The deck and the body under it, in courses along the span.
    let step = 0.25;
    let count = (t.at.bridge_half * 2.0 / step).round() as i32;
    for i in 0..count {
        let (u0, u1) = (
            -t.at.bridge_half + i as f32 * step,
            -t.at.bridge_half + (i + 1) as f32 * step,
        );
        let (y0, y1) = (t.deck_y(u0), t.deck_y(u1));
        // Three flags across, the middle one carrying the groove.
        for (w0, w1) in [(-hw, -0.2), (-0.2, 0.2), (0.2, hw)] {
            let col = pal
                .limestone
                .lerp(pal.limestone_shade, rng.f() * 0.3)
                * rng.value(0.05);
            mb.face(
                [
                    at(u0, w0, y0),
                    at(u1, w0, y1),
                    at(u1, w1, y1),
                    at(u0, w1, y0),
                ],
                Vec3::Y,
                col,
            );
        }
        let (b0, b1) =
            (under(u0).min(y0 - 0.12), under(u1).min(y1 - 0.12));
        for side in [-1.0f32, 1.0] {
            let w = side * hw;
            let col = pal
                .limestone
                .lerp(pal.limestone_shade, 0.25 + rng.f() * 0.3)
                * rng.value(0.05);
            let shade = col * 0.62;
            mb.face_cols(
                [
                    at(u0, w, b0),
                    at(u1, w, b1),
                    at(u1, w, y1),
                    at(u0, w, y0),
                ],
                n * side,
                [shade, shade, col, col],
            );
        }
        let dark = pal.limestone_shade * 0.45;
        mb.face(
            [
                at(u0, -hw, b0),
                at(u1, -hw, b1),
                at(u1, hw, b1),
                at(u0, hw, b0),
            ],
            -Vec3::Y,
            dark,
        );
    }
    for (u, dir) in
        [(-t.at.bridge_half, -1.0), (t.at.bridge_half, 1.0)]
    {
        let y = t.deck_y(u);
        mb.face(
            [
                at(u, -hw, -0.7),
                at(u, hw, -0.7),
                at(u, hw, y),
                at(u, -hw, y),
            ],
            tan * dir,
            pal.limestone_shade,
        );
    }
    // Voussoirs around the arch on both faces.
    let stones = 9;
    for side in [-1.0f32, 1.0] {
        for k in 0..stones {
            let a = (k as f32 + 0.5) / stones as f32 * PI;
            let r = t.at.arch_r + 0.1;
            let p = at(
                -a.cos() * r,
                side * (hw + 0.012),
                t.at.water_y - 0.05 + a.sin() * r * 0.92,
            );
            let basis =
                Mat3::from_cols(tan, Vec3::Y, tan.cross(Vec3::Y));
            let rot = Quat::from_mat3(&basis)
                * Quat::from_rotation_z(FRAC_PI_2 - a)
                * rng.wobble(1.5);
            let col = pal
                .limestone
                .lerp(pal.limestone_shade, rng.f() * 0.25)
                * rng.value(0.06);
            mb.rbox(
                p,
                Vec3::new(0.155, 0.12, 0.05),
                0.03,
                rot,
                col,
                0.1,
            );
        }
    }
    // Parapets: a low course of blocks that follows the hump.
    for side in [-1.0f32, 1.0] {
        let mut u = -t.at.bridge_half - 0.1;
        while u < t.at.bridge_half {
            let len = rng.range(0.34, 0.5);
            let mid = u + len * 0.5;
            let slope =
                (t.deck_y(mid + 0.1) - t.deck_y(mid - 0.1)) / 0.2;
            let basis =
                Mat3::from_cols(tan, Vec3::Y, tan.cross(Vec3::Y));
            let rot = Quat::from_mat3(&basis)
                * Quat::from_rotation_z(slope.atan())
                * rng.wobble(2.0);
            let h = rng.range(0.13, 0.16);
            let col = pal.limestone * rng.value(0.07);
            mb.rbox(
                at(mid, side * (hw - 0.06), t.deck_y(mid) + h - 0.02),
                Vec3::new(len * 0.5 - 0.012, h, 0.11),
                0.04,
                rot,
                col,
                0.25,
            );
            u += len;
        }
    }
}

fn doorstep(mb: &mut Mb, t: &Terrain, pal: &Palette) {
    let mut rng = Rng::seeded("web-search doorstep");
    // Two slabs with the groove's seam between them.
    for side in [-1.0f32, 1.0] {
        let half = Vec3::new(
            t.at.doorstep_half.x * 0.5 - 0.05,
            0.11,
            t.at.doorstep_half.y,
        );
        let c = t.at.doorstep
            + Vec3::new(
                side * (t.at.doorstep_half.x * 0.5 + 0.045),
                t.at.step_top - 0.11,
                0.0,
            );
        mb.rbox(
            c,
            half,
            0.035,
            rng.wobble(0.8),
            pal.limestone * rng.value(0.05),
            0.3,
        );
    }
}
