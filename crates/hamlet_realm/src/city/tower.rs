//! The Hand's tower as meshes: a tall stone shaft with a gallery, and
//! over it an observatory under a dome, with a lantern on its peak.
//! Its +Z is the way its door faces.

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::prelude::*;

use crate::meshes::Meshing;
use crate::palette::Palette;

/// The radius of the ground the tower keeps level and clear.
pub(super) const GROUNDS: f32 = 11.0;

/// The height of the tower to the peak of its dome, and of its
/// lantern over that.
pub(super) const HEIGHT: f32 = 29.0;
pub(super) const LANTERN: f32 = 31.8;

/// The radius of the shaft at its foot, which is also the reach of
/// the box the tower is picked by.
pub(super) const FOOT: f32 = 3.6;

/// The radius of the lowest step of its footing, the widest it is.
pub(super) const FOOTING: f32 = 6.4;

/// The shaft's radius at a height.
fn shaft(y: f32) -> f32 {
    let (foot, top, tall) = (FOOT, 2.7, 21.5);
    foot + (top - foot) * ((y - 1.0) / (tall - 1.0)).clamp(0.0, 1.0)
}

/// A band of `stone` round the shaft, `out` wider than it, from `y`
/// up `tall`.
fn band(meshing: &mut Meshing, y: f32, tall: f32, out: f32, c: Vec3) {
    let (low, high) = (shaft(y) + out, shaft(y + tall) + out);
    meshing.lathe(
        Vec3::ZERO,
        &[
            (low - out, y),
            (low, y + 0.06),
            (high, y + tall - 0.06),
            (high - out, y + tall),
        ],
        16,
        |_, _| c,
    );
}

/// The stone and brass of the tower, and apart from them the glass
/// of its windows and its lantern, which burn while the Hand works.
pub(super) fn build(pal: &Palette) -> (Mesh, Mesh) {
    let (mut meshing, mut glass) = (Meshing::new(), Meshing::new());
    let (stone, shade) = (pal.limestone, pal.limestone_shade);
    // A footing of two steps, and the shaft rising from it.
    meshing.lathe(
        Vec3::ZERO,
        &[
            (FOOTING, 0.0),
            (FOOTING, 0.4),
            (5.2, 0.4),
            (5.2, 0.9),
            (4.4, 0.9),
        ],
        20,
        |r, s| shade.lerp(stone, ((r + s) % 3) as f32 * 0.2),
    );
    let course = |r: usize, s: usize| {
        stone.lerp(shade, ((r * 7 + s * 3) % 5) as f32 * 0.07)
    };
    let rings = [1.0, 6.0, 11.0, 16.0, 21.5];
    for pair in rings.windows(2) {
        meshing.lathe(
            Vec3::ZERO,
            &[(shaft(pair[0]), pair[0]), (shaft(pair[1]), pair[1])],
            16,
            course,
        );
    }
    // A course of darker stone at each stage.
    for y in [6.0, 11.0, 16.0] {
        band(&mut meshing, y - 0.2, 0.4, 0.14, shade);
    }
    // The door, under an arch, and the steps before it.
    meshing.rbox(
        Vec3::new(0.0, 1.9, shaft(1.9) - 0.04),
        Vec3::new(0.9, 1.1, 0.12),
        0.04,
        Quat::IDENTITY,
        pal.timber_dark,
        0.0,
    );
    meshing.rbox(
        Vec3::new(0.0, 3.05, shaft(3.0) - 0.02),
        Vec3::new(1.15, 0.12, 0.2),
        0.03,
        Quat::IDENTITY,
        shade,
        0.0,
    );
    for step in 0..3 {
        let step = step as f32;
        meshing.rbox(
            Vec3::new(0.0, 0.7 - step * 0.2, 4.3 + step * 0.5),
            Vec3::new(1.5, 0.1, 0.3),
            0.02,
            Quat::IDENTITY,
            stone,
            0.0,
        );
    }
    // Narrow windows up the shaft, each stage turned a little from
    // the one below, with a lit light in each.
    for (k, y) in [5.0f32, 9.5, 14.0, 18.5].into_iter().enumerate() {
        for side in 0..2 {
            let a =
                k as f32 * 1.1 + side as f32 * std::f32::consts::PI;
            let turn = Quat::from_rotation_y(a);
            // The frame stands well out of the faceted wall, and the
            // glass out of the frame, so that no face of one lies on
            // a face of another.
            let out = shaft(y);
            meshing.rbox(
                turn * Vec3::new(0.0, y, out + 0.02),
                Vec3::new(0.34, 1.0, 0.14),
                0.03,
                turn,
                pal.timber_dark,
                0.0,
            );
            glass.rbox(
                turn * Vec3::new(0.0, y, out + 0.19),
                Vec3::new(0.24, 0.86, 0.03),
                0.1,
                turn,
                Vec3::ONE,
                0.0,
            );
        }
    }
    // The gallery on corbels, with a rail about it.
    meshing.lathe(
        Vec3::ZERO,
        &[
            (shaft(21.5), 21.0),
            (4.2, 21.9),
            (4.5, 22.0),
            (4.5, 22.5),
            (4.2, 22.5),
        ],
        20,
        |r, _| if r < 2 { shade } else { stone },
    );
    let posts = 16;
    let post = |k: usize| {
        let a = k as f32 / posts as f32 * TAU;
        Vec3::new(a.cos() * 4.3, 22.5, a.sin() * 4.3)
    };
    for k in 0..posts {
        meshing.tube(
            post(k),
            post(k) + Vec3::Y * 1.1,
            0.07,
            0.06,
            6,
            pal.steel,
            true,
        );
        meshing.tube(
            post(k) + Vec3::Y * 1.1,
            post(k + 1) + Vec3::Y * 1.1,
            0.05,
            0.05,
            5,
            pal.steel,
            false,
        );
    }
    // The observatory's drum with a ring of lit lights, and its
    // cornice.
    meshing.lathe(
        Vec3::ZERO,
        &[
            (3.0, 22.5),
            (3.0, 26.0),
            (3.3, 26.1),
            (3.3, 26.5),
            (3.0, 26.5),
        ],
        16,
        |r, s| {
            if r >= 1 {
                shade
            } else {
                stone.lerp(shade, (s % 2) as f32 * 0.1)
            }
        },
    );
    for k in 0..8 {
        let a = (k as f32 + 0.5) / 8.0 * TAU;
        let turn = Quat::from_rotation_y(a);
        meshing.rbox(
            turn * Vec3::new(0.0, 24.3, 3.0 + 0.02),
            Vec3::new(0.5, 0.8, 0.12),
            0.04,
            turn,
            pal.timber_dark,
            0.0,
        );
        glass.rbox(
            turn * Vec3::new(0.0, 24.3, 3.0 + 0.17),
            Vec3::new(0.4, 0.68, 0.03),
            0.12,
            turn,
            Vec3::ONE,
            0.0,
        );
    }
    // The dome, with brass ribs, a slit that looks out and the
    // telescope through it.
    let dome = (0..=7)
        .map(|k| {
            let a = k as f32 / 7.0 * FRAC_PI_2;
            (3.3 * a.cos(), 26.5 + 2.5 * a.sin())
        })
        .collect::<Vec<_>>();
    meshing.lathe(Vec3::ZERO, &dome, 16, |_, s| {
        pal.slate_roof.lerp(shade, (s % 2) as f32 * 0.08)
    });
    for k in 0..8 {
        let a = k as f32 / 8.0 * TAU;
        let rib = (0..=6)
            .map(|j| {
                let t = j as f32 / 6.0 * FRAC_PI_2;
                Vec3::new(
                    a.cos() * 3.36 * t.cos(),
                    26.5 + 2.56 * t.sin(),
                    a.sin() * 3.36 * t.cos(),
                )
            })
            .collect::<Vec<_>>();
        for pair in rib.windows(2) {
            meshing.tube(
                pair[0], pair[1], 0.06, 0.06, 5, pal.brass, false,
            );
        }
    }
    meshing.rbox(
        Vec3::new(0.0, 27.6, 2.55),
        Vec3::new(0.5, 1.3, 0.2),
        0.05,
        Quat::from_rotation_x(-0.5),
        pal.timber_dark,
        0.0,
    );
    meshing.tube(
        Vec3::new(0.0, 27.1, 2.4),
        Vec3::new(0.0, 28.9, 4.9),
        0.34,
        0.26,
        10,
        pal.brass,
        true,
    );
    meshing.tube(
        Vec3::new(0.0, 27.0, 1.6),
        Vec3::new(0.0, 27.1, 2.5),
        0.4,
        0.36,
        10,
        pal.steel,
        false,
    );
    // The lantern: a brass spire and, on it, a lamp of lit glass.
    meshing.tube(
        Vec3::Y * (HEIGHT - 0.4),
        Vec3::Y * (LANTERN - 0.5),
        0.1,
        0.07,
        6,
        pal.brass,
        true,
    );
    meshing.blob(
        Vec3::Y * (LANTERN - 0.9),
        Vec3::new(0.32, 0.12, 0.32),
        3,
        0.0,
        0.0,
        |_| pal.brass,
    );
    glass.blob(
        Vec3::Y * LANTERN,
        Vec3::splat(0.55),
        4,
        0.0,
        0.0,
        |_| Vec3::ONE,
    );
    meshing.blob(
        Vec3::Y * (LANTERN + 0.65),
        Vec3::new(0.3, 0.14, 0.3),
        3,
        0.0,
        0.0,
        |_| pal.brass,
    );
    meshing.tube(
        Vec3::Y * (LANTERN + 0.7),
        Vec3::Y * (LANTERN + 1.7),
        0.06,
        0.01,
        5,
        pal.brass,
        true,
    );
    (meshing.build(), glass.build())
}
