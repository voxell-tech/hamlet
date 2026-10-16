//! Brannoc's stall of minds: shelves of jars that glow with
//! thought, orbs on chains, a crystal ball and a chalk board of
//! prices.

use std::f32::consts::TAU;

use bevy::prelude::*;

use super::super::{FLOOR, lantern, slab};
use super::{
    DEEP, Dressed, HIGH, LOW, WIDE, WORK, awning, awning_ends,
    backdrop, counter, mast, posts, rail, sign,
};
use crate::meshes::Mb;
use crate::palette::{Palette, StallStyle};

/// The colours thought glows in, a jar to each in turn.
fn thoughts(pal: &Palette, style: StallStyle) -> [Vec3; 5] {
    [
        style.glow,
        pal.wicks[1],
        pal.wicks[2],
        pal.wicks[3],
        pal.wicks[4],
    ]
}

/// A jar of thought on `at`, `tall` high: glowing glass, a brass
/// lid and a band at its neck.
fn jar(
    mb: &mut Mb,
    gleam: &mut Mb,
    pal: &Palette,
    at: Vec3,
    tall: f32,
    glow: Vec3,
) {
    let wide = tall * 0.42;
    gleam.lathe(
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
    mb.tube(
        at + Vec3::Y * tall * 0.84,
        at + Vec3::Y * tall,
        wide * 0.7,
        wide * 0.62,
        8,
        pal.brass,
        true,
    );
    mb.blob(
        at + Vec3::Y * (tall + 0.02),
        Vec3::splat(wide * 0.25),
        2,
        0.0,
        0.0,
        |_| pal.brass,
    );
}

/// The stall.
pub(super) fn build(
    mb: &mut Mb,
    (glass, gleam): (&mut Mb, &mut Mb),
    pal: &Palette,
    style: StallStyle,
) -> Dressed {
    let glows = thoughts(pal, style);
    posts(mb, pal.timber_dark, (LOW, HIGH));
    rail(mb, pal, LOW - 0.08);
    awning(mb, pal, [style.cloth, pal.whitewash]);
    backdrop(mb, style.cloth * 0.7, HIGH);
    // Three shelves of jars along the back, between two sides.
    let back = -DEEP + 0.2;
    for side in [-1.0f32, 1.0] {
        slab(
            mb,
            Vec3::new(side * (WIDE - 0.12), FLOOR + 0.95, back),
            Vec3::new(0.03, 0.95, 0.17),
            pal.timber,
        );
    }
    for (row, y) in [0.55f32, 1.15, 1.72].into_iter().enumerate() {
        slab(
            mb,
            Vec3::new(0.0, FLOOR + y, back),
            Vec3::new(WIDE - 0.12, 0.025, 0.17),
            pal.timber,
        );
        let jars = 7 - row % 2;
        for k in 0..jars {
            let x = -1.05 + 2.1 * (k as f32 + 0.5) / jars as f32;
            let tall = 0.2 + 0.06 * ((k + row) % 3) as f32;
            jar(
                mb,
                gleam,
                pal,
                Vec3::new(x, FLOOR + y + 0.025, back),
                tall,
                glows[(k * 2 + row) % glows.len()],
            );
        }
    }
    let top = counter(mb, pal, style, pal.timber);
    // A crystal ball on a brass foot in the middle of the counter,
    // and a row of small jars either side.
    mb.lathe(
        top,
        &[
            (0.0, 0.0),
            (0.14, 0.0),
            (0.1, 0.06),
            (0.06, 0.12),
            (0.0, 0.12),
        ],
        10,
        |_, _| pal.brass,
    );
    gleam.blob(
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
            mb,
            gleam,
            pal,
            top + Vec3::new(x, 0.0, 0.05 * (k % 2) as f32),
            0.16,
            glows[(k + 2) % glows.len()],
        );
    }
    // Orbs of thought on chains from the rail.
    for (k, x) in [-0.9f32, -0.3, 0.35, 0.95].into_iter().enumerate()
    {
        let from = Vec3::new(x, LOW - 0.08, DEEP);
        let drop = 0.3 + 0.12 * (k % 2) as f32;
        let orb = from - Vec3::Y * drop;
        mb.tube(
            from,
            orb + Vec3::Y * 0.08,
            0.008,
            0.008,
            3,
            pal.steel,
            false,
        );
        mb.torus(
            orb + Vec3::Y * 0.07,
            Vec3::Y,
            0.05,
            0.012,
            8,
            pal.brass,
        );
        gleam.blob(orb, Vec3::splat(0.075), 4, 0.0, 0.0, |_| {
            glows[(k + 1) % glows.len()]
        });
    }
    // A chalk board of prices on an easel by the front.
    let board = Vec3::new(-WIDE - 0.45, FLOOR, DEEP + 0.15);
    let lean =
        Quat::from_rotation_x(-0.2) * Quat::from_rotation_y(0.35);
    for side in [-1.0f32, 1.0] {
        mb.tube(
            board + lean * Vec3::new(side * 0.28, 1.2, 0.0),
            board + Vec3::new(side * 0.32, 0.0, 0.05),
            0.025,
            0.025,
            4,
            pal.timber,
            true,
        );
    }
    mb.tube(
        board + lean * Vec3::new(0.0, 1.1, -0.04),
        board + Vec3::new(0.0, 0.0, -0.45),
        0.02,
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
    mb.rbox(
        face + lean * Vec3::Z * 0.012,
        Vec3::new(0.27, 0.31, 0.02),
        0.0,
        lean,
        pal.bed,
        0.0,
    );
    // Lines of chalk: a name and its price to each.
    for line in 0..5 {
        let y = 0.22 - 0.11 * line as f32;
        let long = 0.09 + 0.04 * ((line * 3) % 4) as f32;
        for (x, w) in [(-0.2 + long * 0.5, long * 0.5), (0.17, 0.05)]
        {
            mb.rbox(
                face + lean * Vec3::new(x, y, 0.034),
                Vec3::new(w, 0.012, 0.004),
                0.0,
                lean,
                pal.whitewash,
                0.0,
            );
        }
    }
    // A great orb on a brass stand, which Brannoc gazes into.
    let stand = WORK;
    mb.lathe(
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
        10,
        |_, _| pal.brass,
    );
    for k in 0..3 {
        let a = k as f32 / 3.0 * TAU;
        mb.tube(
            stand + Vec3::new(a.sin() * 0.11, 0.84, a.cos() * 0.11),
            stand + Vec3::new(a.sin() * 0.15, 0.98, a.cos() * 0.15),
            0.012,
            0.012,
            3,
            pal.brass,
            false,
        );
    }
    gleam.blob(
        stand + Vec3::Y * 1.02,
        Vec3::splat(0.17),
        5,
        0.0,
        0.0,
        |d| pal.wicks[1].lerp(style.glow, 0.5 + 0.5 * d.x),
    );
    // The sign: a jar of gold with thought in it.
    let (face, crest) = sign(
        mb,
        pal,
        style,
        Vec3::new(0.0, LOW + 0.18, DEEP + 0.55),
        LOW - 0.12,
    );
    mb.lathe(
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
    gleam.blob(
        face + Vec3::new(0.0, -0.03, 0.1),
        Vec3::splat(0.06),
        3,
        0.0,
        0.0,
        |_| style.glow,
    );
    for side in [-1.0f32, 1.0] {
        for k in 0..3 {
            gleam.blob(
                face + Vec3::new(
                    side * (0.3 + 0.1 * k as f32),
                    0.08 - 0.08 * k as f32,
                    0.02,
                ),
                Vec3::splat(0.025),
                2,
                0.0,
                0.0,
                |_| style.trim,
            );
        }
    }
    let lamp = lantern(
        mb,
        glass,
        pal,
        Vec3::new(0.0, LOW - 0.08, DEEP + 0.25),
    );
    Dressed {
        lamp,
        mast: mast(mb, pal, Vec3::new(WIDE, HIGH, -DEEP)),
        crest,
        ties: awning_ends(),
        cogs: Vec::new(),
    }
}
