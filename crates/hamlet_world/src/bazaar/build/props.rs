//! What furnishes the bazaar's round besides its stalls: palms in
//! pots, a camel at rest, a rack of rugs and a tea table.

use std::f32::consts::{FRAC_PI_4, PI, TAU};

use bevy::math::Affine3A;
use bevy::prelude::*;

use super::{FLOOR, cloth, slab};
use crate::meshes::Mb;
use crate::palette::Palette;

/// The palms, each with the way it is turned: either side of the
/// gate, and between the backs of the stalls.
pub(in crate::bazaar) const PALMS: [(Vec3, f32); 6] = [
    (Vec3::new(-3.0, 0.0, 10.0), 0.3),
    (Vec3::new(3.0, 0.0, 10.0), 2.4),
    (Vec3::new(4.59, 0.0, -2.65), 2.0),
    (Vec3::new(0.0, 0.0, -5.3), 3.0),
    (Vec3::new(-4.59, 0.0, -2.65), 4.1),
    (Vec3::new(-4.59, 0.0, 2.65), 1.1),
];

/// The camel, lying in the gap between two stalls with its head
/// along the ring.
const CAMEL: (Vec3, f32) = (Vec3::new(-6.32, 0.0, -3.65), -0.524);

/// The ends of the rack of rugs, in the gap between two stalls.
const RACK: (Vec3, Vec3) =
    (Vec3::new(5.93, 0.0, -4.52), Vec3::new(6.88, 0.0, -2.88));

/// The tea table, in the gap between the first two stalls, and the
/// way it turns: its cushions are toward the pavilion.
pub(in crate::bazaar) const TEA: (Vec3, f32) =
    (Vec3::new(6.5, 0.0, 3.75), 4.189);

/// The spot over the teapot's lid, from the tea table's middle.
pub(in crate::bazaar) const TEAPOT: Vec3 =
    Vec3::new(-0.08, FLOOR + 0.42 + 0.3, 0.0);

/// Builds `part` turned `turn` about its own up and set at `at`.
fn placed(
    mb: &mut Mb,
    (at, turn): (Vec3, f32),
    part: impl FnOnce(&mut Mb),
) {
    mb.at(Affine3A::from_rotation_translation(
        Quat::from_rotation_y(turn),
        at,
    ));
    part(mb);
    mb.at(Affine3A::IDENTITY);
}

/// Everything that furnishes the round.
pub(super) fn furnish(mb: &mut Mb, pal: &Palette) {
    for palm_at in PALMS {
        placed(mb, palm_at, |mb| palm(mb, pal));
    }
    placed(mb, CAMEL, |mb| camel(mb, pal));
    rack(mb, pal);
    placed(mb, TEA, |mb| tea(mb, pal));
}

/// A palm in a pot.
fn palm(mb: &mut Mb, pal: &Palette) {
    let pot = [
        (0.2, 0.0),
        (0.32, 0.12),
        (0.34, 0.4),
        (0.3, 0.55),
        (0.0, 0.55),
    ];
    mb.lathe(Vec3::Y * FLOOR, &pot, 12, |ring, _| match ring {
        3 => pal.mud,
        1 => pal.terracotta * 0.85,
        _ => pal.terracotta,
    });
    // A trunk of rings, leaning a little and narrowing.
    let lean = |i: usize| {
        let t = i as f32 / 6.0;
        Vec3::new(0.18 * t * t, FLOOR + 0.5 + 1.85 * t, 0.0)
    };
    for i in 0..6 {
        let shade = if i % 2 == 0 { 1.0 } else { 0.82 };
        mb.tube(
            lean(i),
            lean(i + 1),
            0.1 - 0.006 * i as f32,
            0.085 - 0.006 * i as f32,
            7,
            pal.bark * shade,
            true,
        );
    }
    let top = lean(6);
    for k in 0..3 {
        let a = k as f32 / 3.0 * TAU;
        mb.blob(
            top + Vec3::new(a.sin() * 0.1, -0.1, a.cos() * 0.1),
            Vec3::splat(0.075),
            3,
            0.0,
            0.0,
            |_| pal.timber_dark,
        );
    }
    // Fronds that rise from the crown and droop at their ends.
    let fronds = 8;
    for k in 0..fronds {
        let a =
            (k as f32 + 0.3 * (k % 2) as f32) / fronds as f32 * TAU;
        let out = Vec3::new(a.sin(), 0.0, a.cos());
        let side = out.cross(Vec3::Y);
        let long = 0.85 - 0.12 * (k % 3) as f32;
        let along = |t: f32| {
            top + out * long * t + Vec3::Y * (0.32 * t - 0.75 * t * t)
        };
        let colour = match k % 2 {
            0 => pal.foliage,
            _ => pal.foliage_shade,
        };
        let steps = 4;
        for i in 0..steps {
            let (t0, t1) = (
                i as f32 / steps as f32,
                (i + 1) as f32 / steps as f32,
            );
            let (w0, w1) = (
                (PI * t0).sin() * 0.17 + 0.02,
                (PI * t1).sin() * 0.17,
            );
            cloth(
                mb,
                [
                    along(t0) - side * w0,
                    along(t0) + side * w0,
                    along(t1) + side * w1,
                    along(t1) - side * w1,
                ],
                Vec3::Y,
                colour,
            );
        }
    }
}

/// A camel lying down, its head along +Z, with a saddle cloth and
/// packs on its back.
fn camel(mb: &mut Mb, pal: &Palette) {
    let hide = pal.dirt_shade;
    let dark = hide * 0.78;
    let up = |y: f32| FLOOR + y;
    let ball = |mb: &mut Mb, at: Vec3, r: Vec3, colour: Vec3| {
        mb.blob(at, r, 5, 0.0, 0.0, |_| colour);
    };
    ball(
        mb,
        Vec3::new(0.0, up(0.48), 0.0),
        Vec3::new(0.42, 0.36, 0.8),
        hide,
    );
    ball(
        mb,
        Vec3::new(0.0, up(0.88), -0.12),
        Vec3::new(0.3, 0.3, 0.38),
        hide,
    );
    // Its legs folded under it.
    for side in [-1.0f32, 1.0] {
        for z in [0.45f32, -0.45] {
            ball(
                mb,
                Vec3::new(side * 0.36, up(0.11), z),
                Vec3::new(0.14, 0.11, 0.34),
                dark,
            );
        }
    }
    mb.tube(
        Vec3::new(0.0, up(0.6), 0.62),
        Vec3::new(0.0, up(1.22), 1.02),
        0.17,
        0.11,
        8,
        hide,
        true,
    );
    let head = Vec3::new(0.0, up(1.28), 1.16);
    ball(mb, head, Vec3::new(0.13, 0.14, 0.26), hide);
    ball(
        mb,
        head + Vec3::new(0.0, -0.06, 0.22),
        Vec3::new(0.1, 0.09, 0.12),
        dark,
    );
    for side in [-1.0f32, 1.0] {
        mb.blob(
            head + Vec3::new(side * 0.1, 0.06, 0.04),
            Vec3::splat(0.028),
            3,
            0.0,
            0.0,
            |_| pal.ink,
        );
        mb.tube(
            head + Vec3::new(side * 0.08, 0.1, -0.12),
            head + Vec3::new(side * 0.13, 0.2, -0.16),
            0.035,
            0.01,
            4,
            dark,
            true,
        );
    }
    mb.tube(
        Vec3::new(0.0, up(0.66), -0.78),
        Vec3::new(0.0, up(0.28), -0.92),
        0.03,
        0.05,
        4,
        dark,
        true,
    );
    // The saddle cloth before its hump, draped down its sides, with
    // tassels at its corners.
    slab(
        mb,
        Vec3::new(0.0, up(0.84), 0.3),
        Vec3::new(0.34, 0.03, 0.28),
        pal.awnings[0],
    );
    for side in [-1.0f32, 1.0] {
        let flap = Vec3::new(side * 0.42, up(0.6), 0.3);
        mb.rbox(
            flap,
            Vec3::new(0.02, 0.22, 0.28),
            0.01,
            Quat::from_rotation_z(-side * 0.25),
            pal.awnings[0],
            0.0,
        );
        mb.rbox(
            flap + Vec3::new(side * 0.012, -0.12, 0.0),
            Vec3::new(0.02, 0.05, 0.28),
            0.01,
            Quat::from_rotation_z(-side * 0.25),
            pal.awnings[1],
            0.0,
        );
        for z in [0.04f32, 0.56] {
            mb.blob(
                flap + Vec3::new(side * 0.06, -0.25, z - 0.3),
                Vec3::new(0.03, 0.06, 0.03),
                3,
                0.0,
                0.0,
                |_| pal.awnings[1],
            );
        }
        // A pack on each side behind the hump.
        mb.blob(
            Vec3::new(side * 0.48, up(0.72), -0.42),
            Vec3::new(0.16, 0.22, 0.24),
            4,
            0.1,
            side,
            |_| pal.parchment,
        );
        mb.torus(
            Vec3::new(side * 0.48, up(0.72), -0.42),
            Vec3::Z,
            0.17,
            0.015,
            10,
            pal.leather,
        );
    }
}

/// A rack of rugs hung up to be seen, between two posts.
fn rack(mb: &mut Mb, pal: &Palette) {
    let (a, b) = RACK;
    let top = 2.05;
    for end in [a, b] {
        mb.tube(
            end + Vec3::Y * FLOOR,
            end + Vec3::Y * (top + 0.1),
            0.05,
            0.045,
            6,
            pal.timber,
            true,
        );
    }
    mb.tube(
        a + Vec3::Y * top,
        b + Vec3::Y * top,
        0.03,
        0.03,
        6,
        pal.timber_dark,
        true,
    );
    let along = (b - a).normalize();
    let out = Vec3::Y.cross(along);
    let turn = Quat::from_rotation_y((-along.z).atan2(along.x));
    for k in 0..3 {
        let t = (k as f32 + 0.5) / 3.0;
        let middle = a.lerp(b, t) + Vec3::Y * (top - 0.8);
        let field = pal.awnings[(k + 2) % pal.awnings.len()];
        let border = pal.awnings[(k + 1) % pal.awnings.len()];
        for side in [1.0f32, -1.0] {
            let face = |lift: f32| middle + out * side * lift;
            mb.rbox(
                face(0.0),
                Vec3::new(0.25, 0.75, 0.01),
                0.004,
                turn,
                border,
                0.0,
            );
            mb.rbox(
                face(0.008),
                Vec3::new(0.2, 0.68, 0.01),
                0.004,
                turn,
                field,
                0.0,
            );
            mb.rbox(
                face(0.016),
                Vec3::new(0.11, 0.11, 0.01),
                0.004,
                turn * Quat::from_rotation_z(FRAC_PI_4),
                pal.whitewash,
                0.0,
            );
            for y in [-0.45f32, 0.45] {
                mb.rbox(
                    face(0.016) + Vec3::Y * y,
                    Vec3::new(0.16, 0.03, 0.01),
                    0.004,
                    turn,
                    pal.whitewash,
                    0.0,
                );
            }
        }
    }
}

/// A low table of brass with a pot of tea and its glasses, and
/// cushions about it.
fn tea(mb: &mut Mb, pal: &Palette) {
    mb.tube(
        Vec3::Y * FLOOR,
        Vec3::Y * (FLOOR + 0.42),
        0.12,
        0.06,
        6,
        pal.timber_dark,
        true,
    );
    let tray = Vec3::Y * (FLOOR + 0.42);
    mb.tube(
        tray,
        tray + Vec3::Y * 0.03,
        0.42,
        0.42,
        20,
        pal.brass,
        true,
    );
    mb.torus(
        tray + Vec3::Y * 0.035,
        Vec3::Y,
        0.41,
        0.018,
        20,
        pal.brass * 0.85,
    );
    let pot = tray + Vec3::new(-0.08, 0.15, 0.0);
    mb.blob(pot, Vec3::new(0.12, 0.12, 0.12), 4, 0.0, 0.0, |_| {
        pal.brass
    });
    mb.tube(
        pot + Vec3::new(0.08, -0.02, 0.0),
        pot + Vec3::new(0.2, 0.08, 0.0),
        0.025,
        0.012,
        5,
        pal.brass,
        false,
    );
    mb.blob(
        pot + Vec3::Y * 0.13,
        Vec3::new(0.05, 0.06, 0.05),
        3,
        0.0,
        0.0,
        |_| pal.brass,
    );
    for k in 0..4 {
        let a = 1.2 + k as f32 / 4.0 * 2.8;
        let at =
            tray + Vec3::new(a.sin() * 0.26, 0.03, a.cos() * 0.26);
        mb.tube(
            at,
            at + Vec3::Y * 0.09,
            0.03,
            0.035,
            6,
            pal.ochre,
            true,
        );
    }
    for (k, (x, z)) in [(0.6f32, -0.3f32), (0.1, 0.65), (-0.5, -0.35)]
        .into_iter()
        .enumerate()
    {
        mb.blob(
            Vec3::new(x, FLOOR + 0.09, z),
            Vec3::new(0.24, 0.09, 0.24),
            4,
            0.0,
            0.0,
            |_| pal.awnings[(k + 1) % pal.awnings.len()],
        );
        mb.blob(
            Vec3::new(x, FLOOR + 0.17, z),
            Vec3::splat(0.03),
            2,
            0.0,
            0.0,
            |_| pal.brass,
        );
    }
}
