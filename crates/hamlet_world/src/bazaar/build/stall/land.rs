//! Lady Ashgrove's land agency: a tasselled roof, maps pinned up at
//! the back, a little town on the table, a deed with its seal, and a
//! signpost to the plots.

use bevy::prelude::*;

use super::super::{FLOOR, cloth, lantern, slab};
use super::{
    DEEP, Dressed, LOW, WIDE, WORK, counter, mast, posts, sign,
};
use crate::meshes::Mb;
use crate::palette::{Palette, StallStyle};

/// The height of the roof's peak.
const PEAK: f32 = LOW + 1.0;

/// A house of the little town on `at`, `size` across, under a roof
/// of `roof`, turned `turn`.
fn cottage(
    mb: &mut Mb,
    pal: &Palette,
    at: Vec3,
    size: f32,
    roof: Vec3,
    turn: f32,
) {
    let rot = Quat::from_rotation_y(turn);
    let half = Vec3::new(0.06, 0.045, 0.05) * size;
    mb.rbox(
        at + Vec3::Y * half.y,
        half,
        0.004,
        rot,
        pal.plaster,
        0.1,
    );
    for side in [-1.0f32, 1.0] {
        let tilt = rot * Quat::from_rotation_z(side * 0.6);
        mb.rbox(
            at + rot
                * Vec3::new(
                    side * half.x * 0.5,
                    half.y * 2.0 + half.x * 0.32,
                    0.0,
                ),
            Vec3::new(half.x * 0.62, 0.008, half.z * 1.15),
            0.002,
            tilt,
            roof,
            0.0,
        );
    }
    mb.rbox(
        at + rot * Vec3::new(0.0, half.y * 0.6, half.z + 0.002),
        Vec3::new(0.012, 0.022, 0.002) * size,
        0.0,
        rot,
        pal.timber_dark,
        0.0,
    );
}

/// A map on the board at `at`: land, a river, a road, plots and the
/// pins in it.
fn map(mb: &mut Mb, pal: &Palette, at: Vec3, half: Vec2, salt: f32) {
    slab(mb, at, half.extend(0.006), pal.parchment);
    let face = at + Vec3::Z * 0.008;
    for k in 0..3 {
        let c = Vec2::new(
            (salt * 1.7 + k as f32 * 2.1).sin(),
            (salt * 1.3 + k as f32 * 1.4).cos(),
        ) * half
            * 0.5;
        mb.rbox(
            face + c.extend(0.0),
            (half * Vec2::new(0.32, 0.26)).extend(0.002),
            0.0,
            Quat::from_rotation_z(salt + k as f32),
            [pal.grass, pal.foliage_shade, pal.moss][k],
            0.0,
        );
    }
    // A river that winds down it.
    let steps = 6;
    for k in 0..steps {
        let y =
            |k: usize| half.y * (0.9 - 1.8 * k as f32 / steps as f32);
        let x =
            |k: usize| half.x * 0.35 * (k as f32 * 1.3 + salt).sin();
        mb.tube(
            face + Vec3::new(x(k), y(k), 0.004),
            face + Vec3::new(x(k + 1), y(k + 1), 0.004),
            0.012,
            0.012,
            4,
            pal.water,
            false,
        );
    }
    mb.rbox(
        face + Vec3::Z * 0.004,
        Vec3::new(half.x * 0.9, 0.008, 0.002),
        0.0,
        Quat::from_rotation_z(0.3 * salt.sin()),
        pal.dirt_shade,
        0.0,
    );
    for k in 0..3 {
        let p = Vec2::new(
            (salt * 2.3 + k as f32 * 2.7).sin(),
            (salt * 0.7 + k as f32 * 2.2).cos(),
        ) * half
            * 0.7;
        mb.blob(
            face + p.extend(0.02),
            Vec3::splat(0.022),
            2,
            0.0,
            0.0,
            |_| pal.wax,
        );
    }
}

/// The stall.
pub(super) fn build(
    mb: &mut Mb,
    (glass, gleam): (&mut Mb, &mut Mb),
    pal: &Palette,
    style: StallStyle,
) -> Dressed {
    posts(mb, pal.timber_dark, (LOW, LOW));
    // A roof of four slopes up to a peak, striped from its eaves to
    // the peak, with a scalloped valance and a tassel to each
    // scallop.
    let (x, front, back) = (WIDE + 0.3, DEEP + 0.5, -DEEP - 0.3);
    let corners = [
        Vec3::new(-x, LOW, front),
        Vec3::new(x, LOW, front),
        Vec3::new(x, LOW, back),
        Vec3::new(-x, LOW, back),
    ];
    let peak = Vec3::new(0.0, PEAK, (front + back) * 0.5);
    for side in 0..4 {
        let (a, b) = (corners[side], corners[(side + 1) % 4]);
        let out =
            ((a + b) * 0.5 - peak).with_y(0.0).normalize() + Vec3::Y;
        let strips = 8;
        for i in 0..strips {
            let (p, q) = (
                a.lerp(b, i as f32 / strips as f32),
                a.lerp(b, (i + 1) as f32 / strips as f32),
            );
            cloth(
                mb,
                [p, q, peak, peak],
                out,
                [style.cloth, pal.whitewash][i % 2],
            );
            let tip = (p + q) * 0.5 - Vec3::Y * 0.22
                + out.with_y(0.0) * 0.01;
            cloth(mb, [p, q, tip, tip], out.with_y(0.0), style.cloth);
            mb.blob(
                tip - Vec3::Y * 0.04,
                Vec3::new(0.025, 0.05, 0.025),
                2,
                0.0,
                0.0,
                |_| style.trim,
            );
        }
        mb.tube(a, b, 0.025, 0.025, 5, style.trim, false);
    }
    mb.blob(
        peak + Vec3::Y * 0.05,
        Vec3::splat(0.09),
        3,
        0.0,
        0.0,
        |_| pal.brass,
    );
    // A board at the back with maps pinned to it.
    let board = Vec3::new(0.0, FLOOR + 1.45, -DEEP + 0.05);
    slab(mb, board, Vec3::new(1.3, 0.62, 0.03), pal.timber_dark);
    slab(
        mb,
        board + Vec3::Z * 0.02,
        Vec3::new(1.22, 0.55, 0.03),
        pal.ochre * 0.75,
    );
    for (k, (x, y, w, h)) in [
        (-0.72f32, 0.1f32, 0.4f32, 0.36f32),
        (0.05, -0.05, 0.36, 0.44),
        (0.78, 0.12, 0.34, 0.3),
    ]
    .into_iter()
    .enumerate()
    {
        map(
            mb,
            pal,
            board + Vec3::new(x, y, 0.06),
            Vec2::new(w, h),
            k as f32 * 1.9 + 0.4,
        );
    }
    // The table, with a little town on a green board.
    let top = counter(mb, pal, style, pal.timber_dark);
    let green = top + Vec3::new(-0.25, 0.02, -0.02);
    slab(mb, green, Vec3::new(0.66, 0.02, 0.28), pal.grass_shade);
    slab(
        mb,
        green + Vec3::new(0.0, 0.021, 0.0),
        Vec3::new(0.64, 0.002, 0.04),
        pal.dirt,
    );
    slab(
        mb,
        green + Vec3::new(0.15, 0.021, 0.0),
        Vec3::new(0.04, 0.002, 0.26),
        pal.dirt,
    );
    // A brook across one corner, under a bridge.
    for k in 0..4 {
        let t = |k: usize| k as f32 / 4.0;
        mb.tube(
            green
                + Vec3::new(
                    -0.66 + 0.4 * t(k),
                    0.023,
                    -0.28 + 0.56 * t(k) * t(k),
                ),
            green
                + Vec3::new(
                    -0.66 + 0.4 * t(k + 1),
                    0.023,
                    -0.28 + 0.56 * t(k + 1) * t(k + 1),
                ),
            0.025,
            0.025,
            4,
            pal.water,
            false,
        );
    }
    let ground = green + Vec3::Y * 0.022;
    for (k, (x, z, turn)) in [
        (-0.45f32, 0.15f32, 0.2f32),
        (-0.2, 0.17, -0.1),
        (0.38, 0.14, 0.3),
        (0.45, -0.15, 1.4),
        (-0.05, -0.14, 0.0),
        (-0.3, -0.12, 1.6),
    ]
    .into_iter()
    .enumerate()
    {
        let roof = pal.towns[k % pal.towns.len()].roof;
        cottage(
            mb,
            pal,
            ground + Vec3::new(x, 0.0, z),
            1.0 + 0.2 * (k % 2) as f32,
            roof,
            turn,
        );
    }
    for (x, z) in [
        (0.58f32, 0.08f32),
        (0.25, -0.2),
        (-0.55, -0.02),
        (0.05, 0.2),
    ] {
        mb.tube(
            ground + Vec3::new(x, 0.0, z),
            ground + Vec3::new(x, 0.04, z),
            0.008,
            0.008,
            4,
            pal.bark,
            false,
        );
        mb.blob(
            ground + Vec3::new(x, 0.07, z),
            Vec3::new(0.035, 0.045, 0.035),
            3,
            0.0,
            0.0,
            |_| pal.foliage,
        );
    }
    // Its hall in the middle, with a little banner and lit windows.
    let hall = ground + Vec3::new(0.15, 0.0, 0.0);
    mb.tube(
        hall,
        hall + Vec3::Y * 0.02,
        0.06,
        0.06,
        10,
        pal.limestone,
        true,
    );
    mb.tube(
        hall + Vec3::Y * 0.02,
        hall + Vec3::Y * 0.2,
        0.004,
        0.004,
        3,
        pal.timber_dark,
        false,
    );
    cloth(
        mb,
        [
            hall + Vec3::Y * 0.2,
            hall + Vec3::new(0.06, 0.19, 0.0),
            hall + Vec3::new(0.0, 0.15, 0.0),
            hall + Vec3::new(0.0, 0.15, 0.0),
        ],
        Vec3::Z,
        style.cloth,
    );
    gleam.blob(
        hall + Vec3::Y * 0.025,
        Vec3::splat(0.02),
        2,
        0.0,
        0.0,
        |_| style.glow,
    );
    // A deed with its seal, a ribbon and a stamp for the seal.
    let deed = top + Vec3::new(0.7, 0.008, 0.05);
    let turn = Quat::from_rotation_y(-0.25);
    mb.rbox(
        deed,
        Vec3::new(0.16, 0.004, 0.2),
        0.002,
        turn,
        pal.parchment,
        0.0,
    );
    for line in 0..5 {
        mb.rbox(
            deed + turn
                * Vec3::new(0.0, 0.005, -0.14 + 0.05 * line as f32),
            Vec3::new(0.11 - 0.02 * (line % 2) as f32, 0.002, 0.006),
            0.0,
            turn,
            pal.ink,
            0.0,
        );
    }
    mb.tube(
        deed + turn * Vec3::new(-0.18, 0.025, -0.2),
        deed + turn * Vec3::new(0.18, 0.025, -0.2),
        0.025,
        0.025,
        6,
        pal.parchment,
        true,
    );
    mb.blob(
        deed + turn * Vec3::new(0.08, 0.012, 0.13),
        Vec3::new(0.045, 0.012, 0.045),
        3,
        0.05,
        1.0,
        |_| pal.wax,
    );
    for side in [-1.0f32, 1.0] {
        mb.rbox(
            deed + turn * Vec3::new(0.08 + side * 0.025, 0.006, 0.2),
            Vec3::new(0.012, 0.002, 0.06),
            0.0,
            turn * Quat::from_rotation_y(side * 0.3),
            style.cloth,
            0.0,
        );
    }
    let stamp = top + Vec3::new(0.95, 0.0, -0.18);
    mb.tube(
        stamp,
        stamp + Vec3::Y * 0.03,
        0.035,
        0.035,
        8,
        pal.brass,
        true,
    );
    mb.tube(
        stamp + Vec3::Y * 0.03,
        stamp + Vec3::Y * 0.12,
        0.012,
        0.02,
        6,
        pal.timber_dark,
        true,
    );
    mb.blob(
        stamp + Vec3::Y * 0.14,
        Vec3::splat(0.03),
        3,
        0.0,
        0.0,
        |_| pal.timber_dark,
    );
    // A signpost to the plots for sale.
    let post = WORK;
    mb.tube(
        post,
        post + Vec3::Y * 1.75,
        0.04,
        0.035,
        6,
        pal.timber,
        true,
    );
    for (k, (y, turn)) in
        [(1.5f32, 0.4f32), (1.22, -0.5), (0.96, 1.9)]
            .into_iter()
            .enumerate()
    {
        let rot = Quat::from_rotation_y(turn);
        let colour = [style.cloth, style.trim, pal.whitewash][k];
        let mid = post + Vec3::Y * y + rot * Vec3::X * 0.22;
        mb.rbox(
            mid,
            Vec3::new(0.2, 0.07, 0.015),
            0.01,
            rot,
            colour,
            0.0,
        );
        cloth(
            mb,
            [
                mid + rot * Vec3::new(0.2, 0.07, 0.0),
                mid + rot * Vec3::new(0.2, -0.07, 0.0),
                mid + rot * Vec3::new(0.3, 0.0, 0.0),
                mid + rot * Vec3::new(0.3, 0.0, 0.0),
            ],
            rot * Vec3::Z,
            colour,
        );
    }
    mb.blob(
        post + Vec3::Y * 1.78,
        Vec3::splat(0.05),
        3,
        0.0,
        0.0,
        |_| pal.brass,
    );
    // The sign: a house of its trim with lit windows.
    let (face, crest) = sign(
        mb,
        pal,
        style,
        Vec3::new(0.0, LOW + 0.35, front + 0.05),
        LOW,
    );
    mb.rbox(
        face + Vec3::new(0.0, -0.06, 0.0),
        Vec3::new(0.13, 0.09, 0.025),
        0.01,
        Quat::IDENTITY,
        style.trim,
        0.0,
    );
    for side in [-1.0f32, 1.0] {
        mb.rbox(
            face + Vec3::new(side * 0.07, 0.08, 0.0),
            Vec3::new(0.1, 0.02, 0.028),
            0.005,
            Quat::from_rotation_z(-side * 0.65),
            style.trim,
            0.0,
        );
        gleam.blob(
            face + Vec3::new(side * 0.06, -0.05, 0.03),
            Vec3::new(0.025, 0.03, 0.01),
            2,
            0.0,
            0.0,
            |_| style.glow,
        );
    }
    let lamp = lantern(
        mb,
        glass,
        pal,
        Vec3::new(-0.6, LOW + 0.25, DEEP + 0.15),
    );
    Dressed {
        lamp,
        mast: mast(mb, pal, peak + Vec3::Y * 0.1),
        crest,
        ties: [corners[0], corners[1]],
        cogs: Vec::new(),
    }
}
