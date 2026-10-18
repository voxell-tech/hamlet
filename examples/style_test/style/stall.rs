//! The style test's skill stall and its signal post.

use std::f32::consts::PI;

use bevy::math::Affine3A;
use bevy::prelude::*;
use hamlet_assets::tint::hex;
use hamlet_realm::lantern::lantern_frame;
use hamlet_realm::meshes::{Mb, Rng, noise2, smoothstep};
use hamlet_realm::palette::Palette;

use super::scene::{Built, Terrain};

pub(super) fn plank(
    mb: &mut Mb,
    rng: &mut Rng,
    c: Vec3,
    half: Vec3,
    rot: Quat,
    col: Vec3,
) {
    let col = col * rng.value(0.1);
    mb.rbox(c, half, 0.02, rot * rng.wobble(0.9), col, 0.3);
}

/// The skill stall: timber frame, striped awning, counter, bench,
/// sign and lamp.
pub(super) fn stall(b: &mut Built, t: &Terrain, pal: &Palette) {
    let mut rng = Rng::seeded("web-search");
    let xf = Affine3A::from_rotation_translation(
        Quat::from_rotation_y(-0.05),
        t.at.stall,
    );
    b.solid.at(xf);
    b.cloth.at(xf);
    b.iron.at(xf);
    b.brass.at(xf);
    let (hx, hz) = (1.38, 0.82);
    let (front_h, back_h) = (2.02, 2.5);

    // Posts lean a little, each its own way.
    for (x, z, h) in [
        (-hx, hz, front_h),
        (hx, hz, front_h),
        (-hx, -hz, back_h),
        (hx, -hz, back_h),
    ] {
        let rot = rng.wobble(1.4);
        let col = pal.timber * rng.value(0.08);
        b.solid.rbox(
            Vec3::new(x, h * 0.5, z),
            Vec3::new(0.075, h * 0.5, 0.075),
            0.022,
            rot,
            col,
            0.35,
        );
        b.solid.rbox(
            Vec3::new(x, 0.06, z),
            Vec3::new(0.14, 0.06, 0.14),
            0.03,
            rng.wobble(2.0),
            pal.limestone * rng.value(0.06),
            0.3,
        );
    }
    // Top plates and the rafters that carry the canvas.
    plank(
        &mut b.solid,
        &mut rng,
        Vec3::new(0.0, front_h, hz),
        Vec3::new(hx + 0.16, 0.06, 0.06),
        Quat::IDENTITY,
        pal.timber,
    );
    plank(
        &mut b.solid,
        &mut rng,
        Vec3::new(0.0, back_h, -hz),
        Vec3::new(hx + 0.16, 0.06, 0.06),
        Quat::IDENTITY,
        pal.timber,
    );
    let pitch = ((back_h - front_h) / (2.0 * hz)).atan();
    for x in [-hx, 0.0, hx] {
        let rot = Quat::from_rotation_x(pitch);
        plank(
            &mut b.solid,
            &mut rng,
            Vec3::new(x, (front_h + back_h) * 0.5 - 0.02, 0.1),
            Vec3::new(0.045, 0.05, hz + 0.42),
            rot,
            pal.timber_dark,
        );
    }
    // Knee braces at the front corners.
    for side in [-1.0f32, 1.0] {
        let rot = Quat::from_rotation_z(side * 0.78);
        plank(
            &mut b.solid,
            &mut rng,
            Vec3::new(side * (hx - 0.2), front_h - 0.22, hz),
            Vec3::new(0.035, 0.26, 0.035),
            rot,
            pal.timber_dark,
        );
    }
    // Back wall of upright boards, and low sides.
    let boards = 11;
    for i in 0..boards {
        let x = -hx
            + 0.13
            + i as f32 * (2.0 * hx - 0.26) / (boards - 1) as f32;
        let h = 1.95 + rng.sym(0.05);
        plank(
            &mut b.solid,
            &mut rng,
            Vec3::new(x, h * 0.5 + 0.05, -hz),
            Vec3::new(0.118, h * 0.5, 0.025),
            Quat::IDENTITY,
            pal.timber,
        );
    }
    for side in [-1.0f32, 1.0] {
        for i in 0..6 {
            let z = -hz + 0.14 + i as f32 * 0.27;
            let h = 0.92 + rng.sym(0.04);
            plank(
                &mut b.solid,
                &mut rng,
                Vec3::new(side * hx, h * 0.5 + 0.05, z),
                Vec3::new(0.025, h * 0.5, 0.125),
                Quat::IDENTITY,
                pal.timber,
            );
        }
    }
    // Counter across the front, with a gap of boards below.
    plank(
        &mut b.solid,
        &mut rng,
        Vec3::new(0.0, 0.9, hz - 0.08),
        Vec3::new(hx + 0.1, 0.04, 0.27),
        Quat::IDENTITY,
        hex("#8E6A46"),
    );
    for i in 0..10 {
        let x = -hx + 0.2 + i as f32 * (2.0 * hx - 0.4) / 9.0;
        plank(
            &mut b.solid,
            &mut rng,
            Vec3::new(x, 0.46, hz - 0.02),
            Vec3::new(0.125, 0.4, 0.02),
            Quat::IDENTITY,
            pal.timber,
        );
    }
    // A shelf on the back wall.
    plank(
        &mut b.solid,
        &mut rng,
        Vec3::new(0.0, 1.45, -hz + 0.16),
        Vec3::new(hx - 0.15, 0.025, 0.13),
        Quat::IDENTITY,
        pal.timber_dark,
    );

    // Goods: jars and bottles on the shelf, a crate and a scroll rack
    // on the counter.
    let jar_colours = [
        hex("#C9713A"),
        pal.slate_roof,
        pal.moss,
        hex("#EBD9A8"),
        pal.terracotta,
    ];
    for i in 0..7 {
        let x = -hx + 0.42 + i as f32 * 0.36 + rng.sym(0.04);
        let s = rng.range(0.8, 1.15);
        let col =
            jar_colours[i % jar_colours.len()] * rng.value(0.08);
        let profile = [
            (0.0, 0.0),
            (0.07 * s, 0.0),
            (0.09 * s, 0.07 * s),
            (0.085 * s, 0.17 * s),
            (0.045 * s, 0.22 * s),
            (0.05 * s, 0.26 * s),
            (0.0, 0.26 * s),
        ];
        b.solid.lathe(
            Vec3::new(x, 1.475, -hz + 0.16),
            &profile,
            10,
            |_, _| col,
        );
    }
    b.solid.rbox(
        Vec3::new(-0.82, 1.07, hz - 0.1),
        Vec3::new(0.2, 0.13, 0.15),
        0.02,
        Quat::from_rotation_y(0.2),
        pal.timber * 1.25,
        0.3,
    );
    for i in 0..5 {
        let p = Vec3::new(
            -0.92 + i as f32 * 0.05,
            1.23 + (i % 2) as f32 * 0.03,
            hz - 0.1,
        );
        b.solid.tube(
            p - Vec3::Z * 0.12,
            p + Vec3::Z * 0.14,
            0.03,
            0.03,
            8,
            pal.parchment * rng.value(0.06),
            true,
        );
    }
    for i in 0..3 {
        let profile = [
            (0.0, 0.0),
            (0.1, 0.0),
            (0.12, 0.08),
            (0.11, 0.2),
            (0.06, 0.24),
            (0.0, 0.24),
        ];
        let col = jar_colours[(i + 2) % jar_colours.len()];
        b.solid.lathe(
            Vec3::new(
                0.35 + i as f32 * 0.3 + rng.sym(0.03),
                0.94,
                hz - 0.1 + rng.sym(0.05),
            ),
            &profile,
            10,
            |_, _| col,
        );
    }

    awning(&mut b.cloth, &mut rng, pal, hx, hz, front_h, back_h);

    // The signboard hangs from an arm off the left front post.
    let arm_y = 1.72;
    plank(
        &mut b.solid,
        &mut rng,
        Vec3::new(-hx - 0.42, arm_y, hz),
        Vec3::new(0.46, 0.03, 0.03),
        Quat::IDENTITY,
        pal.timber_dark,
    );
    let board = Vec3::new(-hx - 0.48, arm_y - 0.38, hz);
    for x in [-0.2, 0.2] {
        b.iron.tube(
            Vec3::new(board.x + x, arm_y, hz),
            Vec3::new(board.x + x, arm_y - 0.14, hz),
            0.012,
            0.012,
            6,
            pal.bed,
            false,
        );
    }
    let tilt = Quat::from_rotation_z(0.03);
    b.solid.rbox(
        board,
        Vec3::new(0.34, 0.25, 0.03),
        0.02,
        tilt,
        pal.timber,
        0.15,
    );
    b.solid.rbox(
        board + Vec3::Z * 0.012,
        Vec3::new(0.29, 0.2, 0.028),
        0.012,
        tilt,
        pal.parchment,
        0.05,
    );
    // A painted lens: the stall's trade without a word.
    let glyph = board + Vec3::new(-0.04, 0.03, 0.045);
    b.solid.torus(glyph, Vec3::Z, 0.085, 0.017, 18, pal.ink);
    b.solid.tube(
        glyph + Vec3::new(0.065, -0.065, 0.0),
        glyph + Vec3::new(0.17, -0.17, 0.0),
        0.02,
        0.02,
        6,
        pal.ink,
        true,
    );

    // The lamp's hook: an iron arm off the right front post.
    let hook = Vec3::new(hx, 1.86, hz + 0.07);
    b.iron.tube(
        hook,
        hook + Vec3::new(0.0, 0.06, 0.34),
        0.016,
        0.016,
        6,
        pal.bed,
        true,
    );
    b.iron.tube(
        hook + Vec3::new(0.0, -0.2, 0.0),
        hook + Vec3::new(0.0, 0.04, 0.2),
        0.012,
        0.012,
        6,
        pal.bed,
        false,
    );
    b.iron.tube(
        hook + Vec3::new(0.0, 0.06, 0.32),
        hook + Vec3::new(0.0, -0.08, 0.32),
        0.008,
        0.008,
        6,
        pal.bed,
        false,
    );
    b.brass.at(Affine3A::IDENTITY);
    lantern_frame(
        &mut b.brass,
        t.at.stall_lamp(),
        0.085,
        0.2,
        pal.brass,
    );

    // A bench beside the stall, and a barrel and crate to anchor the
    // corner.
    b.solid.at(Affine3A::IDENTITY);
    b.cloth.at(Affine3A::IDENTITY);
    b.iron.at(Affine3A::IDENTITY);
    let bench =
        Vec3::new(t.at.stall.x - 2.35, 0.0, t.at.stall.z + 1.15);
    let yaw = Quat::from_rotation_y(0.22);
    plank(
        &mut b.solid,
        &mut rng,
        bench + Vec3::Y * 0.42,
        Vec3::new(0.6, 0.035, 0.17),
        yaw,
        hex("#8E6A46"),
    );
    for side in [-1.0f32, 1.0] {
        let leg = bench + yaw * Vec3::new(side * 0.45, 0.2, 0.0);
        plank(
            &mut b.solid,
            &mut rng,
            leg,
            Vec3::new(0.04, 0.2, 0.14),
            yaw,
            pal.timber_dark,
        );
    }
    let barrel =
        Vec3::new(t.at.stall.x + 1.95, 0.0, t.at.stall.z + 0.55);
    let staves = [
        (0.0, 0.0),
        (0.2, 0.0),
        (0.25, 0.2),
        (0.26, 0.34),
        (0.25, 0.48),
        (0.2, 0.68),
        (0.0, 0.68),
    ];
    let timber = pal.timber;
    b.solid.lathe(barrel, &staves, 14, |_, s| {
        timber * (1.1 + 0.12 * ((s * 7 % 5) as f32 / 5.0 - 0.5))
    });
    for y in [0.14, 0.54] {
        b.iron.torus(
            barrel + Vec3::Y * y,
            Vec3::Y,
            0.245,
            0.014,
            16,
            pal.bed,
        );
    }
    b.solid.rbox(
        barrel + Vec3::new(0.52, 0.17, 0.28),
        Vec3::splat(0.17),
        0.02,
        Quat::from_rotation_y(0.4),
        pal.timber * 1.2,
        0.35,
    );
}

fn awning(
    mb: &mut Mb,
    rng: &mut Rng,
    pal: &Palette,
    hx: f32,
    hz: f32,
    front_h: f32,
    back_h: f32,
) {
    let stripes = 9;
    let half_w = hx + 0.3;
    let (z_back, z_front) = (-hz - 0.22, hz + 0.55);
    let slope = (back_h - front_h) / (2.0 * hz);
    let surface = |u: f32, v: f32| {
        let x = u * half_w;
        let z = z_back + (z_front - z_back) * v;
        // Canvas hangs between the three rafters and dips toward the
        // front edge.
        let between = (u * PI).cos().abs();
        let sag = 0.05 * (v * PI).sin()
            + 0.045 * (1.0 - between) * (0.4 + 0.6 * v);
        Vec3::new(x, back_h + 0.17 - slope * (z + hz) - sag, z)
    };
    let normal = |u: f32, v: f32| {
        let e = 0.01;
        (surface(u, v + e) - surface(u, v - e))
            .cross(surface(u + e, v) - surface(u - e, v))
            .normalize()
    };
    let faded = hex("#B5553C").lerp(pal.parchment, 0.12);
    for s in 0..stripes {
        let col = if s % 2 == 0 { pal.parchment } else { faded }
            * rng.value(0.04);
        let (u0, u1) = (
            -1.0 + 2.0 * s as f32 / stripes as f32,
            -1.0 + 2.0 * (s + 1) as f32 / stripes as f32,
        );
        let (cols, rows) = (3, 8);
        let mut ids = vec![vec![0u32; rows + 1]; cols + 1];
        for (i, column) in ids.iter_mut().enumerate() {
            for (j, id) in column.iter_mut().enumerate() {
                let (u, v) = (
                    u0 + (u1 - u0) * i as f32 / cols as f32,
                    j as f32 / rows as f32,
                );
                // Weathering: paler along the ridge, grubby toward
                // the drip edge.
                let shade = 1.0 - 0.14 * smoothstep(0.55, 1.0, v)
                    + 0.04 * (noise2(u * 9.0, v * 5.0) - 0.5);
                *id = mb.v(surface(u, v), normal(u, v), col * shade);
            }
        }
        for i in 0..cols {
            for j in 0..rows {
                mb.quad(
                    ids[i][j],
                    ids[i][j + 1],
                    ids[i + 1][j + 1],
                    ids[i + 1][j],
                );
            }
        }
        // A scalloped valance hangs from the front edge.
        let (a, c) = (surface(u0, 1.0), surface(u1, 1.0));
        let m = (a + c) * 0.5;
        let n = Vec3::new(0.0, 0.25, 1.0).normalize();
        let drop = Vec3::new(0.0, -0.12, 0.012);
        let col = col * 0.86;
        let v = [a, c, c + drop, a + drop, m + drop * 2.0]
            .map(|p| mb.v(p, n, col));
        mb.quad(v[0], v[1], v[2], v[3]);
        mb.tri(v[3], v[2], v[4]);
    }
}

/// The semaphore post. The arm and the lamp glass are spawned
/// separately so they can move and light.
pub(super) fn signal_post(b: &mut Built, t: &Terrain, pal: &Palette) {
    let mut rng = Rng::seeded("signal post: web-search");
    let p = t.at.signal_post;
    b.solid.rbox(
        p + Vec3::Y * 0.09,
        Vec3::new(0.2, 0.09, 0.2),
        0.035,
        rng.wobble(2.0),
        pal.limestone * rng.value(0.05),
        0.3,
    );
    let lean = rng.wobble(1.2);
    b.solid.rbox(
        p + Vec3::Y * 1.2,
        Vec3::new(0.06, 1.1, 0.06),
        0.02,
        lean,
        pal.timber * rng.value(0.06),
        0.3,
    );
    b.solid.rbox(
        p + Vec3::Y * 2.3,
        Vec3::new(0.085, 0.03, 0.085),
        0.015,
        lean,
        pal.timber_dark,
        0.1,
    );
    // Two pegs: vetoes given here before.
    for (i, y) in [0.95f32, 1.08].into_iter().enumerate() {
        let side = if i == 0 { 1.0 } else { -1.0 };
        b.solid.tube(
            p + Vec3::new(0.0, y, 0.05),
            p + Vec3::new(side * 0.02, y + 0.02, 0.15),
            0.014,
            0.012,
            6,
            pal.timber_dark,
            true,
        );
    }
    b.brass.tube(
        p + Vec3::new(0.0, t.at.signal_arm_y, 0.06),
        p + Vec3::new(0.0, t.at.signal_arm_y, 0.11),
        0.028,
        0.028,
        8,
        pal.brass,
        true,
    );
    lantern_frame(
        &mut b.brass,
        p + Vec3::Y * t.at.signal_lamp_y,
        0.075,
        0.15,
        pal.brass,
    );
}

/// The semaphore arm, pivoting about its origin and pointing along
/// +X.
pub fn signal_arm(pal: &Palette) -> Mesh {
    let mut mb = Mb::new();
    let orange = hex("#C96A1A");
    mb.rbox(
        Vec3::new(0.3, 0.0, 0.0),
        Vec3::new(0.36, 0.055, 0.015),
        0.012,
        Quat::IDENTITY,
        pal.parchment,
        0.0,
    );
    mb.rbox(
        Vec3::new(0.6, 0.0, 0.0),
        Vec3::new(0.1, 0.085, 0.017),
        0.012,
        Quat::IDENTITY,
        orange,
        0.0,
    );
    mb.rbox(
        Vec3::new(0.34, 0.0, 0.004),
        Vec3::new(0.035, 0.057, 0.015),
        0.006,
        Quat::IDENTITY,
        orange,
        0.0,
    );
    mb.rbox(
        Vec3::new(-0.12, 0.0, 0.0),
        Vec3::new(0.08, 0.04, 0.02),
        0.012,
        Quat::IDENTITY,
        pal.bed,
        0.0,
    );
    mb.build()
}
