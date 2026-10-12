//! The style test's cottage, there so that the night has windows.

use std::f32::consts::{FRAC_PI_2, PI};

use bevy::math::Affine3A;
use bevy::prelude::*;
use hamlet_assets::tint::hex;
use hamlet_world::meshes::{Rng, noise2};
use hamlet_world::palette::Palette;

use super::scene::{Built, Terrain};
use super::stall::plank;

/// A small house for the backdrop, so that the night has windows.
pub(super) fn cottage(b: &mut Built, t: &Terrain, pal: &Palette) {
    let mut rng = Rng::seeded("the neighbour's cottage");
    let xf = Affine3A::from_rotation_translation(
        Quat::from_rotation_y(0.14),
        t.at.cottage,
    );
    b.solid.at(xf);
    b.window.at(xf);
    let (hx, hz) = (1.75, 1.4);
    let (base_h, wall_h, ridge_h) = (0.7, 2.25, 3.5);

    b.solid.rbox(
        Vec3::new(0.0, base_h * 0.5, 0.0),
        Vec3::new(hx, base_h * 0.5, hz),
        0.05,
        Quat::IDENTITY,
        pal.limestone_shade.lerp(pal.limestone, 0.6),
        0.35,
    );
    let lean = rng.wobble(0.6);
    b.solid.rbox(
        Vec3::new(0.0, (base_h + wall_h) * 0.5, 0.0),
        Vec3::new(
            hx - 0.05,
            (wall_h - base_h) * 0.5 + 0.02,
            hz - 0.05,
        ),
        0.04,
        lean,
        pal.ochre,
        0.12,
    );
    // Foundation blocks and quoins, each a little off.
    let mut x = -hx;
    while x < hx - 0.1 {
        let len = rng.range(0.4, 0.62).min(hx - x);
        for z in [hz + 0.02, -hz - 0.02] {
            let col = pal
                .limestone
                .lerp(pal.limestone_shade, rng.f() * 0.4)
                * rng.value(0.05);
            b.solid.rbox(
                Vec3::new(x + len * 0.5, base_h * 0.5, z),
                Vec3::new(
                    len * 0.5 - 0.012,
                    base_h * 0.5 + rng.sym(0.03),
                    0.06,
                ),
                0.04,
                rng.wobble(1.5),
                col,
                0.4,
            );
        }
        x += len;
    }
    for (sx, sz) in
        [(-1.0f32, 1.0f32), (1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)]
    {
        let mut y = 0.14;
        let mut k = 0;
        while y < wall_h - 0.1 {
            let h = rng.range(0.12, 0.16);
            let long = if k % 2 == 0 {
                (0.24, 0.14)
            } else {
                (0.14, 0.24)
            };
            let c = Vec3::new(
                sx * (hx - long.0 + 0.04),
                y,
                sz * (hz - long.1 + 0.04),
            );
            let col = pal.limestone * rng.value(0.07);
            b.solid.rbox(
                c,
                Vec3::new(long.0, h, long.1),
                0.035,
                rng.wobble(2.0),
                col,
                0.15,
            );
            y += h * 2.0 + 0.01;
            k += 1;
        }
    }
    // Worn patches where the ochre has come away.
    for _ in 0..7 {
        let (w, h) = (rng.range(0.18, 0.42), rng.range(0.12, 0.3));
        let c = Vec3::new(
            rng.sym(hx - 0.6),
            rng.range(base_h + 0.25, wall_h - 0.3),
            hz - 0.045,
        );
        let col = hex("#B9C08A").lerp(pal.plaster, rng.f());
        b.solid.rbox(
            c,
            Vec3::new(w, h, 0.012),
            0.01,
            Quat::from_rotation_z(rng.sym(0.3)),
            col,
            0.0,
        );
    }
    // Gables.
    for side in [-1.0f32, 1.0] {
        let x = side * (hx - 0.06);
        let pts = [
            Vec3::new(x, wall_h, -hz + 0.05),
            Vec3::new(x, wall_h, hz - 0.05),
            Vec3::new(x, ridge_h - 0.06, 0.0),
            Vec3::new(x, ridge_h - 0.06, 0.0),
        ];
        b.solid.face(pts, Vec3::X * side, pal.ochre * 0.97);
    }
    // Tiles in rows, with drift in hue and moss on the north slope.
    let rows = 8;
    let per_row = 13;
    let slope_len =
        ((ridge_h - wall_h).powi(2) + (hz + 0.3) * (hz + 0.3)).sqrt();
    let pitch = ((ridge_h - wall_h) / (hz + 0.3)).atan();
    for side in [-1.0f32, 1.0] {
        for r in 0..rows {
            let k = (r as f32 + 0.5) / rows as f32;
            let z = side * (hz + 0.3) * (1.0 - k);
            let offset =
                rng.sym(0.03) + if r % 2 == 0 { 0.0 } else { 0.5 };
            for i in 0..per_row + 1 {
                let w = (2.0 * hx + 0.5) / per_row as f32;
                let x = -hx - 0.25 + (i as f32 + offset) * w;
                if x > hx + 0.3 || x < -hx - 0.3 {
                    continue;
                }
                let sag = 0.08 * (x / hx * FRAC_PI_2).cos() * k;
                let y = wall_h - 0.06 + (ridge_h - wall_h) * k - sag
                    + 0.03;
                let mut col = pal
                    .terracotta
                    .lerp(pal.tile_drift, rng.f() * 0.7)
                    * rng.value(0.08);
                let mossy = noise2(x * 1.4 + 20.0, k * 3.0 + side)
                    + if side < 0.0 { 0.2 } else { -0.12 };
                if mossy > 0.62 {
                    col = col.lerp(pal.moss, rng.range(0.5, 0.9));
                }
                let rot = Quat::from_rotation_x(side * pitch)
                    * Quat::from_rotation_x(side * -0.07)
                    * rng.wobble(1.6);
                b.solid.rbox(
                    Vec3::new(x, y, z),
                    Vec3::new(
                        w * 0.5 - 0.008,
                        0.028,
                        slope_len / rows as f32 * 0.62,
                    ),
                    0.016,
                    rot,
                    col,
                    0.0,
                );
            }
        }
    }
    // Ridge course.
    let mut x = -hx - 0.25;
    while x < hx + 0.2 {
        let sag = 0.08 * (x / hx * FRAC_PI_2).cos();
        let col = pal.terracotta * rng.value(0.1) * 0.9;
        b.solid.rbox(
            Vec3::new(x + 0.15, ridge_h - sag + 0.02, 0.0),
            Vec3::new(0.16, 0.05, 0.09),
            0.035,
            rng.wobble(2.5),
            col,
            0.0,
        );
        x += 0.31;
    }
    // Chimney.
    let mut y = ridge_h - 0.75;
    while y < ridge_h + 0.55 {
        let col = pal.limestone * rng.value(0.08);
        b.solid.rbox(
            Vec3::new(hx - 0.55, y, -0.35),
            Vec3::new(0.2, 0.085, 0.2),
            0.03,
            rng.wobble(2.0),
            col,
            0.1,
        );
        y += 0.16;
    }
    b.solid.rbox(
        Vec3::new(hx - 0.55, y + 0.01, -0.35),
        Vec3::new(0.24, 0.035, 0.24),
        0.02,
        Quat::IDENTITY,
        pal.limestone_shade,
        0.0,
    );

    // Door with a stone surround.
    let door = Vec3::new(-0.55, 0.0, hz);
    for i in 0..4 {
        let x = door.x - 0.23 + i as f32 * 0.153;
        plank(
            &mut b.solid,
            &mut rng,
            Vec3::new(x, 0.72, hz + 0.0),
            Vec3::new(0.074, 0.62, 0.035),
            Quat::IDENTITY,
            pal.timber,
        );
    }
    for side in [-1.0f32, 1.0] {
        for k in 0..5 {
            let col = pal.limestone * rng.value(0.07);
            b.solid.rbox(
                door + Vec3::new(
                    side * 0.4,
                    0.2 + k as f32 * 0.28,
                    0.03,
                ),
                Vec3::new(0.09, 0.135, 0.07),
                0.03,
                rng.wobble(2.0),
                col,
                0.1,
            );
        }
    }
    for k in 0..5 {
        let a = (k as f32 + 0.5) / 5.0 * PI;
        let p = door
            + Vec3::new(-a.cos() * 0.4, 1.42 + a.sin() * 0.2, 0.03);
        let col = pal.limestone * rng.value(0.07);
        b.solid.rbox(
            p,
            Vec3::new(0.13, 0.09, 0.07),
            0.03,
            Quat::from_rotation_z(FRAC_PI_2 - a) * rng.wobble(1.5),
            col,
            0.1,
        );
    }
    b.solid.rbox(
        door + Vec3::new(0.0, 0.05, 0.3),
        Vec3::new(0.5, 0.06, 0.26),
        0.03,
        rng.wobble(1.0),
        pal.limestone,
        0.3,
    );
    b.iron.at(xf);
    b.iron.torus(
        door + Vec3::new(0.14, 0.72, 0.045),
        Vec3::Z,
        0.045,
        0.01,
        10,
        pal.bed,
    );
    b.iron.at(Affine3A::IDENTITY);

    // Lattice windows with shutters.
    for (x, y) in [(0.75, 1.55), (0.75, 0.0), (-0.55, 0.0)] {
        if y == 0.0 {
            continue;
        }
        window(
            b,
            &mut rng,
            pal,
            Vec3::new(x, y, hz - 0.03),
            Quat::IDENTITY,
        );
    }
    window(
        b,
        &mut rng,
        pal,
        Vec3::new(-0.75, 2.0, hz - 0.03),
        Quat::IDENTITY,
    );
    window(
        b,
        &mut rng,
        pal,
        Vec3::new(-hx + 0.03, 1.6, 0.2),
        Quat::from_rotation_y(-FRAC_PI_2),
    );
    b.solid.at(Affine3A::IDENTITY);
    b.window.at(Affine3A::IDENTITY);
}

fn window(
    b: &mut Built,
    rng: &mut Rng,
    pal: &Palette,
    c: Vec3,
    face: Quat,
) {
    let (w, h) = (0.26, 0.34);
    let at = |x: f32, y: f32, z: f32| c + face * Vec3::new(x, y, z);
    let n = face * Vec3::Z;
    b.window.face(
        [
            at(-w, -h, 0.0),
            at(w, -h, 0.0),
            at(w, h, 0.0),
            at(-w, h, 0.0),
        ],
        n,
        Vec3::ONE,
    );
    // Frame, mullion and a diamond lattice.
    let frame = pal.timber_dark;
    for (x, y, hw, hh) in [
        (-w, 0.0, 0.03, h + 0.03),
        (w, 0.0, 0.03, h + 0.03),
        (0.0, h, w, 0.03),
        (0.0, -h, w + 0.05, 0.04),
        (0.0, 0.0, 0.018, h),
    ] {
        b.solid.rbox(
            at(x, y, 0.02),
            Vec3::new(hw, hh, 0.035),
            0.012,
            face,
            frame,
            0.0,
        );
    }
    for k in -2..=2 {
        for dir in [-1.0f32, 1.0] {
            let rot = face * Quat::from_rotation_z(dir * 0.7);
            b.solid.rbox(
                at(k as f32 * 0.13, 0.0, 0.006),
                Vec3::new(0.007, h * 1.25, 0.006),
                0.002,
                rot,
                pal.ink,
                0.0,
            );
        }
    }
    for side in [-1.0f32, 1.0] {
        let col = pal.slate_roof * rng.value(0.08);
        let rot = face * Quat::from_rotation_y(side * -0.25);
        b.solid.rbox(
            at(side * (w + 0.17), 0.0, 0.05),
            Vec3::new(0.125, h + 0.02, 0.02),
            0.012,
            rot,
            col,
            0.1,
        );
    }
}
