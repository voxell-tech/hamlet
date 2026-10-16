//! Sister Marjoram's stall of skill-scrolls: a roof like a chapel's,
//! racks of scrolls, scrolls hung open from its front and a lectern
//! with a book, a quill and its ink.

use bevy::math::Affine3A;
use bevy::prelude::*;

use super::super::{FLOOR, cloth, lantern, slab};
use super::{
    DEEP, Dressed, LOW, WIDE, WORK, counter, mast, posts, sign,
    within,
};
use crate::meshes::Mb;
use crate::palette::{Palette, StallStyle};

/// The height of the roof's ridge.
const RIDGE: f32 = LOW + 0.85;

/// The lectern: where it stands and the height of its desk.
const LECTERN: (Vec3, f32) = (WORK, 1.05);

/// The spot over the lectern's book that a shimmer rises from.
pub(in crate::bazaar) const SHIMMER: Vec3 = Vec3::new(
    LECTERN.0.x,
    LECTERN.0.y + LECTERN.1 + 0.12,
    LECTERN.0.z,
);

/// A rolled scroll from `a` to `b` of `radius`, with knobs at its
/// ends and a ribbon of `tie` round its middle.
fn scroll(
    mb: &mut Mb,
    pal: &Palette,
    (a, b): (Vec3, Vec3),
    radius: f32,
    tie: Vec3,
) {
    mb.tube(a, b, radius, radius, 7, pal.parchment, true);
    let along = (b - a).normalize_or(Vec3::X);
    for end in [a, b] {
        let out = if end == a { -along } else { along };
        mb.blob(
            end + out * 0.02,
            Vec3::splat(radius * 0.7),
            2,
            0.0,
            0.0,
            |_| pal.timber_dark,
        );
    }
    mb.torus((a + b) * 0.5, along, radius + 0.004, 0.008, 8, tie);
}

/// A scroll hung open from `from`, `long` and `wide`, with lines of
/// ink on its face.
fn hung(
    mb: &mut Mb,
    pal: &Palette,
    from: Vec3,
    (wide, long): (f32, f32),
    salt: usize,
) {
    for y in [0.0, -long] {
        mb.tube(
            from + Vec3::new(-wide - 0.04, y, 0.0),
            from + Vec3::new(wide + 0.04, y, 0.0),
            0.018,
            0.018,
            5,
            pal.timber_dark,
            true,
        );
    }
    let middle = from - Vec3::Y * long * 0.5;
    mb.rbox(
        middle,
        Vec3::new(wide, long * 0.5, 0.006),
        0.003,
        Quat::IDENTITY,
        pal.parchment,
        0.0,
    );
    let lines = (long / 0.07) as usize - 2;
    for line in 0..lines {
        let w = wide
            * (0.75 - 0.2 * ((line * 7 + salt) % 4) as f32 / 3.0);
        for out in [1.0f32, -1.0] {
            mb.rbox(
                from + Vec3::new(
                    -wide * 0.8 + w,
                    -0.1 - 0.07 * line as f32,
                    out * 0.008,
                ),
                Vec3::new(w, 0.008, 0.003),
                0.0,
                Quat::IDENTITY,
                pal.ink,
                0.0,
            );
        }
    }
    // A seal at its foot.
    mb.blob(
        from + Vec3::new(wide * 0.6, -long + 0.08, 0.012),
        Vec3::new(0.035, 0.035, 0.012),
        3,
        0.0,
        0.0,
        |_| pal.wax,
    );
}

/// The stall.
pub(super) fn build(
    mb: &mut Mb,
    (glass, gleam): (&mut Mb, &mut Mb),
    pal: &Palette,
    style: StallStyle,
) -> Dressed {
    posts(mb, pal.timber_dark, (LOW, LOW));
    // A roof of two slopes on a ridge from side to side, striped down
    // each slope, with its gables closed in the trim.
    let edge = WIDE + 0.3;
    let (front, back) =
        ((DEEP + 0.5, LOW - 0.12), (-DEEP - 0.35, LOW - 0.12));
    let strips = 9;
    for i in 0..strips {
        let (x0, x1) = (
            -edge + 2.0 * edge * i as f32 / strips as f32,
            -edge + 2.0 * edge * (i + 1) as f32 / strips as f32,
        );
        let colour = [style.cloth, style.trim][i % 2];
        for (z, y, out) in
            [(front.0, front.1, 1.0f32), (back.0, back.1, -1.0)]
        {
            cloth(
                mb,
                [
                    Vec3::new(x0, RIDGE, 0.0),
                    Vec3::new(x1, RIDGE, 0.0),
                    Vec3::new(x1, y, z),
                    Vec3::new(x0, y, z),
                ],
                Vec3::new(0.0, 1.0, out),
                colour,
            );
        }
    }
    for side in [-1.0f32, 1.0] {
        cloth(
            mb,
            [
                Vec3::new(side * edge, front.1, front.0),
                Vec3::new(side * edge, back.1, back.0),
                Vec3::new(side * edge, RIDGE, 0.0),
                Vec3::new(side * edge, RIDGE, 0.0),
            ],
            Vec3::X * side,
            style.cloth * 0.85,
        );
        // A finial with a little bell at each end of the ridge.
        let tip = Vec3::new(side * (edge + 0.02), RIDGE, 0.0);
        mb.tube(
            tip,
            tip + Vec3::Y * 0.3,
            0.03,
            0.01,
            6,
            pal.brass,
            true,
        );
        mb.blob(
            tip + Vec3::Y * 0.32,
            Vec3::splat(0.045),
            3,
            0.0,
            0.0,
            |_| pal.brass,
        );
    }
    mb.tube(
        Vec3::new(-edge, RIDGE + 0.01, 0.0),
        Vec3::new(edge, RIDGE + 0.01, 0.0),
        0.035,
        0.035,
        6,
        pal.timber_dark,
        true,
    );
    // A rack of pigeonholes along the back, a scroll in each.
    let rack = Vec3::new(0.0, FLOOR, -DEEP + 0.15);
    let (cols, rows) = (6, 4);
    let cell = Vec2::new(2.3 / cols as f32, 0.36);
    slab(
        mb,
        rack + Vec3::new(0.0, 0.85, -0.13),
        Vec3::new(1.2, 0.85, 0.02),
        pal.timber,
    );
    for c in 0..=cols {
        let x = -1.15 + cell.x * c as f32;
        slab(
            mb,
            rack + Vec3::new(x, 0.85, 0.0),
            Vec3::new(0.02, 0.85, 0.14),
            pal.timber,
        );
    }
    for r in 0..=rows {
        let y = 0.2 + cell.y * r as f32;
        slab(
            mb,
            rack + Vec3::new(0.0, y, 0.0),
            Vec3::new(1.17, 0.02, 0.14),
            pal.timber,
        );
    }
    for c in 0..cols {
        for r in 0..rows {
            let x = -1.15 + cell.x * (c as f32 + 0.5);
            let y = 0.2 + cell.y * r as f32 + 0.02;
            let n = (c * 5 + r * 3) % 4;
            let tie = [style.cloth, pal.wax, pal.moss, style.trim][n];
            // Two or three to a hole, sticking out a little.
            for k in 0..(2 + n % 2) {
                let at = rack
                    + Vec3::new(
                        x - 0.07 + 0.07 * k as f32,
                        y + 0.05 + 0.06 * (k % 2) as f32,
                        0.0,
                    );
                scroll(
                    mb,
                    pal,
                    (
                        at - Vec3::Z * 0.12,
                        at + Vec3::Z * (0.18 + 0.05 * k as f32),
                    ),
                    0.035,
                    tie,
                );
            }
        }
    }
    // A low counter with scrolls heaped on it and one spread open.
    let top = counter(mb, pal, style, pal.timber);
    for (k, (y, z)) in [
        (0.0f32, -0.1f32),
        (0.0, 0.0),
        (0.0, 0.1),
        (0.08, -0.05),
        (0.08, 0.05),
        (0.16, 0.0),
    ]
    .into_iter()
    .enumerate()
    {
        let at = top + Vec3::new(-0.55, 0.04 + y, z);
        scroll(
            mb,
            pal,
            (at - Vec3::X * 0.3, at + Vec3::X * 0.3),
            0.04,
            [style.cloth, pal.wax, style.trim][k % 3],
        );
    }
    let open = top + Vec3::new(0.35, 0.012, 0.02);
    mb.rbox(
        open,
        Vec3::new(0.32, 0.006, 0.2),
        0.003,
        Quat::from_rotation_y(0.1),
        pal.parchment,
        0.0,
    );
    for line in 0..5 {
        mb.rbox(
            open + Vec3::new(
                -0.05 + 0.03 * (line % 2) as f32,
                0.008,
                -0.13 + 0.065 * line as f32,
            ),
            Vec3::new(0.2, 0.004, 0.008),
            0.0,
            Quat::from_rotation_y(0.1),
            pal.ink,
            0.0,
        );
    }
    for x in [-0.3f32, 0.3] {
        mb.tube(
            open + Vec3::new(x, 0.03, -0.22),
            open + Vec3::new(x, 0.03, 0.22),
            0.03,
            0.03,
            6,
            pal.parchment,
            true,
        );
    }
    // Scrolls hung open from the front.
    let rail = Vec3::new(0.0, LOW - 0.1, DEEP);
    mb.tube(
        rail - Vec3::X * WIDE,
        rail + Vec3::X * WIDE,
        0.022,
        0.022,
        5,
        pal.timber_dark,
        true,
    );
    for (k, x) in [-0.85f32, 0.0, 0.85].into_iter().enumerate() {
        let from = rail + Vec3::new(x, -0.06, 0.0);
        mb.tube(
            rail + Vec3::X * x,
            from,
            0.006,
            0.006,
            3,
            pal.steel,
            false,
        );
        hung(mb, pal, from, (0.2, 0.55 + 0.1 * (k % 2) as f32), k);
    }
    // The lectern: a post on a foot, a sloping desk, a book open on
    // it, an ink pot and its quill, and a candle.
    let (stand, desk) = LECTERN;
    mb.lathe(
        stand,
        &[
            (0.0, 0.0),
            (0.24, 0.0),
            (0.22, 0.06),
            (0.06, 0.12),
            (0.05, desk - 0.08),
            (0.0, desk - 0.06),
        ],
        8,
        |_, _| pal.timber,
    );
    let yaw = Quat::from_rotation_y(-1.15);
    let slope = yaw * Quat::from_rotation_x(0.45);
    let face = stand + Vec3::Y * desk;
    mb.rbox(
        face,
        Vec3::new(0.26, 0.025, 0.2),
        0.01,
        slope,
        pal.timber_dark,
        0.0,
    );
    for side in [-1.0f32, 1.0] {
        mb.rbox(
            face + slope * Vec3::new(side * 0.12, 0.04, 0.0),
            Vec3::new(0.11, 0.012, 0.15),
            0.004,
            slope * Quat::from_rotation_z(-side * 0.12),
            pal.page,
            0.0,
        );
        for line in 0..4 {
            mb.rbox(
                face + slope
                    * Vec3::new(
                        side * 0.12,
                        0.054,
                        -0.09 + 0.06 * line as f32,
                    ),
                Vec3::new(0.08, 0.002, 0.006),
                0.0,
                slope * Quat::from_rotation_z(-side * 0.12),
                pal.ink,
                0.0,
            );
        }
    }
    // A ledge along its high edge for the ink and the candle.
    let ledge = face + yaw * Vec3::new(0.0, 0.1, -0.24);
    mb.rbox(
        ledge,
        Vec3::new(0.27, 0.015, 0.06),
        0.005,
        yaw,
        pal.timber_dark,
        0.0,
    );
    let ink = ledge + yaw * Vec3::new(0.14, 0.055, 0.0);
    mb.tube(
        ink - Vec3::Y * 0.04,
        ink + Vec3::Y * 0.04,
        0.05,
        0.035,
        8,
        pal.ink,
        true,
    );
    let quill = ink + Vec3::Y * 0.03;
    mb.tube(
        quill,
        quill + Vec3::new(0.06, 0.32, -0.04),
        0.008,
        0.004,
        4,
        pal.parchment,
        false,
    );
    within(
        mb,
        Affine3A::from_translation(
            quill + Vec3::new(0.045, 0.24, -0.03),
        ),
        |mb| {
            mb.blob(
                Vec3::ZERO,
                Vec3::new(0.035, 0.14, 0.012),
                4,
                0.1,
                2.0,
                |d| {
                    pal.whitewash.lerp(
                        style.trim,
                        (0.5 - d.y * 0.5).clamp(0.0, 1.0),
                    )
                },
            );
        },
    );
    let candle = ledge + yaw * Vec3::new(-0.16, 0.015, 0.0);
    mb.tube(
        candle,
        candle + Vec3::Y * 0.14,
        0.03,
        0.03,
        8,
        pal.page,
        true,
    );
    gleam.blob(
        candle + Vec3::Y * 0.18,
        Vec3::new(0.02, 0.04, 0.02),
        3,
        0.0,
        0.0,
        |_| pal.ember,
    );
    // The sign: a scroll with a quill across it.
    let (face, crest) = sign(
        mb,
        pal,
        style,
        Vec3::new(0.0, LOW + 0.25, DEEP + 0.62),
        front.1,
    );
    scroll(
        mb,
        pal,
        (
            face + Vec3::new(-0.3, 0.0, 0.03),
            face + Vec3::new(0.3, 0.0, 0.03),
        ),
        0.06,
        style.cloth,
    );
    mb.tube(
        face + Vec3::new(-0.15, -0.12, 0.08),
        face + Vec3::new(0.25, 0.14, 0.08),
        0.012,
        0.004,
        4,
        style.trim,
        false,
    );
    let lamp = lantern(
        mb,
        glass,
        pal,
        Vec3::new(0.42, LOW + 0.02, DEEP + 0.3),
    );
    Dressed {
        lamp,
        mast: mast(mb, pal, Vec3::new(0.0, RIDGE, 0.0)),
        crest,
        ties: [-1.0f32, 1.0].map(|side| {
            Vec3::new(side * edge, front.1 - 0.02, front.0)
        }),
        cogs: Vec::new(),
    }
}
