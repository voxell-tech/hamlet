//! Brannoc's stall of minds: shelves of jars that glow with thought,
//! orbs on chains, a crystal ball and a chalk board of prices.

use std::f32::consts::TAU;

use bevy::prelude::*;

use super::super::{FLOOR, Shapes};
use super::{
    DEEP, Dressed, HIGH, LOW, Stand, WIDE, WORK, awning_ends,
};
use crate::meshes::Meshing;

/// A jar of thought on `at`, `tall` high: glowing glass, a brass
/// lid and a knob on it.
fn jar(s: &mut Stand, at: Vec3, tall: f32, glow: Vec3) {
    let wide = tall * 0.42;
    s.gleam.lathe(
        at,
        &[
            (0.0, 0.0),
            (wide * 0.8, 0.0),
            (wide, tall * 0.18),
            (wide, tall * 0.7),
            (wide * 0.62, tall * 0.86),
            (0.0, tall * 0.86),
        ],
        8,
        |_, _| glow,
    );
    s.mb.tube(
        at + Vec3::Y * tall * 0.84,
        at + Vec3::Y * tall,
        wide * 0.7,
        wide * 0.62,
        8,
        s.pal.brass,
        true,
    );
    s.mb.ball(
        at + Vec3::Y * (tall + 0.02),
        Vec3::splat(wide * 0.25),
        2,
        s.pal.brass,
    );
}

/// A brass stand turned from `profile` on `at`.
fn turned(
    mb: &mut Meshing,
    at: Vec3,
    profile: &[(f32, f32)],
    c: Vec3,
) {
    mb.lathe(at, profile, 10, |_, _| c);
}

pub(super) fn build(s: &mut Stand) -> Dressed {
    let (pal, style) = (s.pal, s.style);
    // The colours thought glows in, a jar to each in turn.
    let glows = [
        style.glow,
        pal.wicks[1],
        pal.wicks[2],
        pal.wicks[3],
        pal.wicks[4],
    ];
    s.posts(pal.timber_dark, (LOW, HIGH));
    s.rail(LOW - 0.08);
    s.awning([style.cloth, pal.whitewash]);
    s.backdrop(style.cloth * 0.7, HIGH);
    // Three shelves of jars along the back, between two sides.
    let back = -DEEP + 0.2;
    for side in [-1.0f32, 1.0] {
        s.mb.slab(
            Vec3::new(side * (WIDE - 0.12), FLOOR + 0.95, back),
            Vec3::new(0.03, 0.95, 0.17),
            pal.timber,
        );
    }
    for (row, y) in [0.55f32, 1.15, 1.72].into_iter().enumerate() {
        s.mb.slab(
            Vec3::new(0.0, FLOOR + y, back),
            Vec3::new(WIDE - 0.12, 0.025, 0.17),
            pal.timber,
        );
        let jars = 7 - row % 2;
        for k in 0..jars {
            let x = -1.05 + 2.1 * (k as f32 + 0.5) / jars as f32;
            let tall = 0.2 + 0.06 * ((k + row) % 3) as f32;
            jar(
                s,
                Vec3::new(x, FLOOR + y + 0.025, back),
                tall,
                glows[(k * 2 + row) % glows.len()],
            );
        }
    }
    let top = s.counter(pal.timber);
    // A crystal ball on a brass foot in the middle of the counter,
    // and a row of small jars either side.
    turned(
        s.mb,
        top,
        &[
            (0.0, 0.0),
            (0.14, 0.0),
            (0.1, 0.06),
            (0.06, 0.12),
            (0.0, 0.12),
        ],
        pal.brass,
    );
    s.gleam.blob(
        top + Vec3::Y * 0.27,
        Vec3::splat(0.17),
        5,
        0.0,
        0.0,
        |d| style.glow.lerp(pal.whitewash, d.y.max(0.0) * 0.5),
    );
    for (k, x) in
        [-0.85f32, -0.65, -0.45, 0.45, 0.65].into_iter().enumerate()
    {
        jar(
            s,
            top + Vec3::new(x, 0.0, 0.05 * (k % 2) as f32),
            0.16,
            glows[(k + 2) % glows.len()],
        );
    }
    // Orbs of thought on chains from the rail.
    for (k, x) in [-0.9f32, -0.3, 0.35, 0.95].into_iter().enumerate()
    {
        let from = Vec3::new(x, LOW - 0.08, DEEP);
        let orb = from - Vec3::Y * (0.3 + 0.12 * (k % 2) as f32);
        s.mb.rod(
            from,
            orb + Vec3::Y * 0.08,
            0.008,
            3,
            pal.steel,
            false,
        );
        s.mb.torus(
            orb + Vec3::Y * 0.07,
            Vec3::Y,
            0.05,
            0.012,
            8,
            pal.brass,
        );
        s.gleam.ball(
            orb,
            Vec3::splat(0.075),
            4,
            glows[(k + 1) % glows.len()],
        );
    }
    board(s);
    // A great orb on a brass stand, which Brannoc gazes into.
    let stand = WORK;
    turned(
        s.mb,
        stand,
        &[
            (0.0, 0.0),
            (0.2, 0.0),
            (0.2, 0.05),
            (0.06, 0.12),
            (0.045, 0.75),
            (0.12, 0.85),
            (0.0, 0.86),
        ],
        pal.brass,
    );
    for k in 0..3 {
        let a = k as f32 / 3.0 * TAU;
        s.mb.rod(
            stand + Vec3::new(a.sin() * 0.11, 0.84, a.cos() * 0.11),
            stand + Vec3::new(a.sin() * 0.15, 0.98, a.cos() * 0.15),
            0.012,
            3,
            pal.brass,
            false,
        );
    }
    s.gleam.blob(
        stand + Vec3::Y * 1.02,
        Vec3::splat(0.17),
        5,
        0.0,
        0.0,
        |d| pal.wicks[1].lerp(style.glow, 0.5 + 0.5 * d.x),
    );
    // The sign: a jar of gold with thought in it.
    let (face, crest) =
        s.sign(Vec3::new(0.0, LOW + 0.18, DEEP + 0.55), LOW - 0.12);
    s.mb.lathe(
        face + Vec3::new(0.0, -0.15, 0.0),
        &[
            (0.0, 0.0),
            (0.09, 0.0),
            (0.12, 0.06),
            (0.12, 0.2),
            (0.07, 0.26),
            (0.0, 0.26),
        ],
        8,
        |_, _| style.trim,
    );
    s.gleam.ball(
        face + Vec3::new(0.0, -0.03, 0.1),
        Vec3::splat(0.06),
        3,
        style.glow,
    );
    for side in [-1.0f32, 1.0] {
        for k in 0..3 {
            s.gleam.ball(
                face + Vec3::new(
                    side * (0.3 + 0.1 * k as f32),
                    0.08 - 0.08 * k as f32,
                    0.02,
                ),
                Vec3::splat(0.025),
                2,
                style.trim,
            );
        }
    }
    s.finish(
        (
            Vec3::new(0.0, LOW - 0.08, DEEP + 0.25),
            Vec3::new(WIDE, HIGH, -DEEP),
        ),
        crest,
        awning_ends(),
        Vec::new(),
    )
}

/// A chalk board of prices on an easel by the front.
fn board(s: &mut Stand) {
    let (mb, pal) = (&mut *s.mb, s.pal);
    let board = Vec3::new(-WIDE - 0.45, FLOOR, DEEP + 0.15);
    let lean =
        Quat::from_rotation_x(-0.2) * Quat::from_rotation_y(0.35);
    for side in [-1.0f32, 1.0] {
        mb.rod(
            board + lean * Vec3::new(side * 0.28, 1.2, 0.0),
            board + Vec3::new(side * 0.32, 0.0, 0.05),
            0.025,
            4,
            pal.timber,
            true,
        );
    }
    mb.rod(
        board + lean * Vec3::new(0.0, 1.1, -0.04),
        board + Vec3::new(0.0, 0.0, -0.45),
        0.02,
        4,
        pal.timber,
        true,
    );
    let face = board + lean * Vec3::new(0.0, 0.82, 0.04);
    mb.rbox(
        face,
        Vec3::new(0.32, 0.36, 0.025),
        0.01,
        lean,
        pal.timber,
        0.0,
    );
    mb.plate(
        face + lean * Vec3::Z * 0.012,
        Vec3::new(0.27, 0.31, 0.02),
        lean,
        pal.bed,
    );
    // Lines of chalk: a name and its price to each.
    for line in 0..5 {
        let y = 0.22 - 0.11 * line as f32;
        let long = 0.09 + 0.04 * ((line * 3) % 4) as f32;
        for (x, w) in [(-0.2 + long * 0.5, long * 0.5), (0.17, 0.05)]
        {
            mb.plate(
                face + lean * Vec3::new(x, y, 0.034),
                Vec3::new(w, 0.012, 0.004),
                lean,
                pal.whitewash,
            );
        }
    }
}
