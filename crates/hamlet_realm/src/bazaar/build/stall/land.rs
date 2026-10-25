//! Lady Ashgrove's land agency: a tasselled roof, maps pinned up at
//! the back, a little town on the table, a deed with its seal, and a
//! signpost to the plots.

use bevy::prelude::*;

use super::super::{FLOOR, Shapes};
use super::{DEEP, Dressed, LOW, Stand, WIDE, WORK};
use crate::meshes::Meshing;
use crate::palette::{Palette, StallStyle};

/// The height of the roof's peak.
const PEAK: f32 = LOW + 1.0;

/// A house of the little town on `at`, `size` across, under a roof
/// of `roof`, turned `turn`.
fn cottage(
    mb: &mut Meshing,
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
        // Each slope falls away from the ridge.
        mb.rbox(
            at + rot
                * Vec3::new(
                    side * half.x * 0.5,
                    half.y * 2.0 + half.x * 0.32,
                    0.0,
                ),
            Vec3::new(half.x * 0.62, 0.008, half.z * 1.15),
            0.002,
            rot * Quat::from_rotation_z(-side * 0.6),
            roof,
            0.0,
        );
    }
    mb.plate(
        at + rot * Vec3::new(0.0, half.y * 0.6, half.z + 0.002),
        Vec3::new(0.012, 0.022, 0.002) * size,
        rot,
        pal.timber_dark,
    );
}

/// A map on the board at `at`: land, a river, a road, plots and the
/// pins in it.
fn map(
    mb: &mut Meshing,
    pal: &Palette,
    at: Vec3,
    half: Vec2,
    salt: f32,
) {
    mb.slab(at, half.extend(0.006), pal.parchment);
    let face = at + Vec3::Z * 0.008;
    for (k, colour) in [pal.grass, pal.foliage_shade, pal.moss]
        .into_iter()
        .enumerate()
    {
        let c = Vec2::new(
            (salt * 1.7 + k as f32 * 2.1).sin(),
            (salt * 1.3 + k as f32 * 1.4).cos(),
        ) * half
            * 0.5;
        mb.plate(
            face + c.extend(0.0),
            (half * Vec2::new(0.32, 0.26)).extend(0.002),
            Quat::from_rotation_z(salt + k as f32),
            colour,
        );
    }
    // A river that winds down it.
    let steps = 6;
    let bend = |k: usize| {
        Vec3::new(
            half.x * 0.35 * (k as f32 * 1.3 + salt).sin(),
            half.y * (0.9 - 1.8 * k as f32 / steps as f32),
            0.004,
        )
    };
    for k in 0..steps {
        mb.rod(
            face + bend(k),
            face + bend(k + 1),
            0.012,
            4,
            pal.water,
            false,
        );
    }
    mb.plate(
        face + Vec3::Z * 0.004,
        Vec3::new(half.x * 0.9, 0.008, 0.002),
        Quat::from_rotation_z(0.3 * salt.sin()),
        pal.dirt_shade,
    );
    for k in 0..3 {
        let p = Vec2::new(
            (salt * 2.3 + k as f32 * 2.7).sin(),
            (salt * 0.7 + k as f32 * 2.2).cos(),
        ) * half
            * 0.7;
        mb.ball(
            face + p.extend(0.02),
            Vec3::splat(0.022),
            2,
            pal.wax,
        );
    }
}

pub(super) fn build(s: &mut Stand) -> Dressed {
    let (pal, style) = (s.pal, s.style);
    s.posts(pal.timber_dark, (LOW, LOW));
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
            s.mb.flap(
                (p, q),
                peak,
                out,
                [style.cloth, pal.whitewash][i % 2],
            );
            let tip = (p + q) * 0.5 - Vec3::Y * 0.22
                + out.with_y(0.0) * 0.01;
            s.mb.flap((p, q), tip, out.with_y(0.0), style.cloth);
            s.mb.ball(
                tip - Vec3::Y * 0.04,
                Vec3::new(0.025, 0.05, 0.025),
                2,
                style.trim,
            );
        }
        s.mb.rod(a, b, 0.025, 5, style.trim, false);
    }
    s.mb.ball(peak + Vec3::Y * 0.05, Vec3::splat(0.09), 3, pal.brass);
    // A board at the back with maps pinned to it.
    let board = Vec3::new(0.0, FLOOR + 1.45, -DEEP + 0.05);
    s.mb.slab(board, Vec3::new(1.3, 0.62, 0.03), pal.timber_dark);
    s.mb.slab(
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
            s.mb,
            pal,
            board + Vec3::new(x, y, 0.06),
            Vec2::new(w, h),
            k as f32 * 1.9 + 0.4,
        );
    }
    let top = s.counter(pal.timber_dark);
    town(s, top);
    deed(s, top);
    signpost(s.mb, pal, s.style);
    // The sign: a house of its trim with lit windows.
    let (face, crest) =
        s.sign(Vec3::new(0.0, LOW + 0.35, front + 0.05), LOW);
    s.mb.rbox(
        face + Vec3::new(0.0, -0.06, 0.0),
        Vec3::new(0.13, 0.09, 0.025),
        0.01,
        Quat::IDENTITY,
        style.trim,
        0.0,
    );
    for side in [-1.0f32, 1.0] {
        s.mb.rbox(
            face + Vec3::new(side * 0.07, 0.08, 0.0),
            Vec3::new(0.1, 0.02, 0.028),
            0.005,
            Quat::from_rotation_z(-side * 0.65),
            style.trim,
            0.0,
        );
        s.gleam.ball(
            face + Vec3::new(side * 0.06, -0.05, 0.03),
            Vec3::new(0.025, 0.03, 0.01),
            2,
            style.glow,
        );
    }
    s.finish(
        (
            Vec3::new(-0.6, LOW + 0.25, DEEP + 0.15),
            peak + Vec3::Y * 0.1,
        ),
        crest,
        [corners[0], corners[1]],
        Vec::new(),
    )
}

/// The little town on a green board on the table's `top`, with a
/// brook under a bridge, trees and its hall in the middle.
fn town(s: &mut Stand, top: Vec3) {
    let (mb, pal, style) = (&mut *s.mb, s.pal, s.style);
    let green = top + Vec3::new(-0.25, 0.02, -0.02);
    mb.slab(green, Vec3::new(0.66, 0.02, 0.28), pal.grass_shade);
    mb.slab(
        green + Vec3::new(0.0, 0.021, 0.0),
        Vec3::new(0.64, 0.002, 0.04),
        pal.dirt,
    );
    mb.slab(
        green + Vec3::new(0.15, 0.021, 0.0),
        Vec3::new(0.04, 0.002, 0.26),
        pal.dirt,
    );
    let brook = |k: usize| {
        let t = k as f32 / 4.0;
        green
            + Vec3::new(-0.66 + 0.4 * t, 0.023, -0.28 + 0.56 * t * t)
    };
    for k in 0..4 {
        mb.rod(brook(k), brook(k + 1), 0.025, 4, pal.water, false);
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
        cottage(
            mb,
            pal,
            ground + Vec3::new(x, 0.0, z),
            1.0 + 0.2 * (k % 2) as f32,
            pal.towns[k % pal.towns.len()].roof,
            turn,
        );
    }
    for (x, z) in [
        (0.58f32, 0.08f32),
        (0.25, -0.2),
        (-0.55, -0.02),
        (0.05, 0.2),
    ] {
        let at = ground + Vec3::new(x, 0.0, z);
        mb.rod(at, at + Vec3::Y * 0.04, 0.008, 4, pal.bark, false);
        mb.ball(
            at + Vec3::Y * 0.07,
            Vec3::new(0.035, 0.045, 0.035),
            3,
            pal.foliage,
        );
    }
    // Its hall in the middle, with a little banner and lit windows.
    let hall = ground + Vec3::new(0.15, 0.0, 0.0);
    mb.rod(
        hall,
        hall + Vec3::Y * 0.02,
        0.06,
        10,
        pal.limestone,
        true,
    );
    mb.rod(
        hall + Vec3::Y * 0.02,
        hall + Vec3::Y * 0.2,
        0.004,
        3,
        pal.timber_dark,
        false,
    );
    mb.flap(
        (hall + Vec3::Y * 0.2, hall + Vec3::new(0.06, 0.19, 0.0)),
        hall + Vec3::new(0.0, 0.15, 0.0),
        Vec3::Z,
        style.cloth,
    );
    s.gleam.ball(
        hall + Vec3::Y * 0.025,
        Vec3::splat(0.02),
        2,
        style.glow,
    );
}

/// A deed on the table's `top` with its seal and a ribbon, a rolled
/// one behind it, and a stamp for the seal.
fn deed(s: &mut Stand, top: Vec3) {
    let (mb, pal, style) = (&mut *s.mb, s.pal, s.style);
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
        mb.plate(
            deed + turn
                * Vec3::new(0.0, 0.005, -0.14 + 0.05 * line as f32),
            Vec3::new(0.11 - 0.02 * (line % 2) as f32, 0.002, 0.006),
            turn,
            pal.ink,
        );
    }
    mb.rod(
        deed + turn * Vec3::new(-0.18, 0.025, -0.2),
        deed + turn * Vec3::new(0.18, 0.025, -0.2),
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
        mb.plate(
            deed + turn * Vec3::new(0.08 + side * 0.025, 0.006, 0.2),
            Vec3::new(0.012, 0.002, 0.06),
            turn * Quat::from_rotation_y(side * 0.3),
            style.cloth,
        );
    }
    let stamp = top + Vec3::new(0.95, 0.0, -0.18);
    mb.rod(stamp, stamp + Vec3::Y * 0.03, 0.035, 8, pal.brass, true);
    mb.tube(
        stamp + Vec3::Y * 0.03,
        stamp + Vec3::Y * 0.12,
        0.012,
        0.02,
        6,
        pal.timber_dark,
        true,
    );
    mb.ball(
        stamp + Vec3::Y * 0.14,
        Vec3::splat(0.03),
        3,
        pal.timber_dark,
    );
}

/// A signpost to the plots for sale, where the keeper works.
fn signpost(mb: &mut Meshing, pal: &Palette, style: StallStyle) {
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
        mb.flap(
            (
                mid + rot * Vec3::new(0.2, 0.07, 0.0),
                mid + rot * Vec3::new(0.2, -0.07, 0.0),
            ),
            mid + rot * Vec3::new(0.3, 0.0, 0.0),
            rot * Vec3::Z,
            colour,
        );
    }
    mb.ball(post + Vec3::Y * 1.78, Vec3::splat(0.05), 3, pal.brass);
}
