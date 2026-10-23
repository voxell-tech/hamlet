//! The library's house as meshes.

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::prelude::*;

use crate::meshes::Meshing;
use crate::palette::Palette;

/// A box of `colour` with its middle at `at`.
fn slab(meshing: &mut Meshing, at: Vec3, half: Vec3, colour: Vec3) {
    meshing.rbox(at, half, 0.03, Quat::IDENTITY, colour, 0.0);
}

/// A window in the wall at `at` that looks along `out`: a dark
/// frame, shelves of books behind it, and lit glass over them.
fn window(
    meshing: &mut Meshing,
    glass: &mut Meshing,
    pal: &Palette,
    (at, out): (Vec3, Vec3),
    salt: usize,
) {
    let turn = Quat::from_rotation_y(out.x.atan2(out.z));
    let across = turn * Vec3::X;
    meshing.rbox(
        at,
        Vec3::new(0.62, 1.25, 0.06),
        0.03,
        turn,
        pal.timber_dark,
        0.0,
    );
    // Three shelves, each a row of spines of every colour. A shelf
    // stands out past its books, and no two books beside each
    // other are as thick, as tall or as far forward, so that no
    // face of one lies in the face of another.
    for shelf in 0..3 {
        let y = -0.95 + shelf as f32 * 0.62;
        meshing.rbox(
            at + Vec3::Y * (y - 0.03) + out * 0.06,
            Vec3::new(0.54, 0.03, 0.065),
            0.0,
            turn,
            pal.timber,
            0.0,
        );
        // Each begins where the last ended.
        let mut left = -0.5;
        for book in 0..7 {
            let mix = salt * 7 + shelf * 5 + book * 3;
            let thick = 0.045 + 0.012 * (mix % 4) as f32;
            if left + thick * 2.0 > 0.5 {
                break;
            }
            let tall = 0.19 + 0.035 * ((mix + book) % 4) as f32;
            let deep = 0.03 + 0.006 * ((mix + shelf) % 3) as f32;
            meshing.rbox(
                at + across * (left + thick)
                    + Vec3::Y * (y + tall + 0.002)
                    + out * (0.045 + deep * 0.5),
                Vec3::new(thick - 0.004, tall, deep),
                0.0,
                turn,
                pal.paints[mix % pal.paints.len()],
                0.0,
            );
            left += thick * 2.0;
        }
    }
    // A lit light over the shelves, round at its top.
    glass.rbox(
        at + Vec3::Y * 1.0 + out * 0.05,
        Vec3::new(0.5, 0.2, 0.03),
        0.1,
        turn,
        Vec3::ONE,
        0.0,
    );
}

/// The stone and timber of the library, and the lit glass of its
/// windows and lamps apart from them.
pub(super) fn build(pal: &Palette) -> (Mesh, Mesh) {
    let (mut meshing, mut glass) = (Meshing::new(), Meshing::new());
    let (stone, shade) = (pal.limestone, pal.limestone_shade);
    // The podium, the porch before it and the steps up.
    slab(
        &mut meshing,
        Vec3::new(0.0, 0.3, 0.0),
        Vec3::new(5.0, 0.3, 3.8),
        shade,
    );
    slab(
        &mut meshing,
        Vec3::new(0.0, 0.3, 4.4),
        Vec3::new(3.9, 0.3, 0.9),
        shade,
    );
    for step in 0..3 {
        let step = step as f32;
        slab(
            &mut meshing,
            Vec3::new(0.0, 0.5 - step * 0.2, 5.55 + step * 0.45),
            Vec3::new(2.9, 0.1, 0.25),
            stone,
        );
    }
    // The hall and the course round its top.
    slab(
        &mut meshing,
        Vec3::new(0.0, 2.7, 0.0),
        Vec3::new(4.2, 2.1, 3.0),
        stone,
    );
    slab(
        &mut meshing,
        Vec3::new(0.0, 4.9, 0.6),
        Vec3::new(4.5, 0.14, 4.0),
        shade,
    );
    // Four columns of the porch, each on a foot and under a head.
    for x in [-3.1f32, -1.15, 1.15, 3.1] {
        let foot = Vec3::new(x, 0.6, 4.6);
        slab(
            &mut meshing,
            foot + Vec3::Y * 0.1,
            Vec3::new(0.36, 0.1, 0.36),
            shade,
        );
        meshing.tube(
            foot + Vec3::Y * 0.2,
            foot + Vec3::Y * 3.95,
            0.27,
            0.23,
            10,
            stone,
            false,
        );
        slab(
            &mut meshing,
            foot + Vec3::Y * 4.05,
            Vec3::new(0.36, 0.1, 0.36),
            shade,
        );
    }
    // The roof, with its ridge from back to front, and the
    // pediment over the porch.
    let (eave, ridge) = (5.04, 6.5);
    let (back, fore, wide) = (-3.4, 5.0, 4.7);
    for side in [-1.0f32, 1.0] {
        meshing.face(
            [
                Vec3::new(side * wide, eave, back),
                Vec3::new(side * wide, eave, fore),
                Vec3::new(0.0, ridge, fore),
                Vec3::new(0.0, ridge, back),
            ],
            Vec3::new(side, 1.0, 0.0),
            pal.slate_roof,
        );
    }
    for (z, out) in [(fore, 1.0f32), (back, -1.0)] {
        meshing.face(
            [
                Vec3::new(-wide, eave, z),
                Vec3::new(wide, eave, z),
                Vec3::new(0.0, ridge, z),
                Vec3::new(0.0, ridge, z),
            ],
            Vec3::Z * out,
            stone,
        );
    }
    // An open book on the pediment, which is what the house is for.
    for side in [-1.0f32, 1.0] {
        meshing.rbox(
            Vec3::new(side * 0.34, 5.52, fore + 0.04),
            Vec3::new(0.32, 0.24, 0.03),
            0.02,
            Quat::from_rotation_z(side * 0.14),
            pal.page,
            0.0,
        );
    }
    slab(
        &mut meshing,
        Vec3::new(0.0, 5.5, fore + 0.03),
        Vec3::new(0.03, 0.27, 0.03),
        pal.leather,
    );
    // The reading room's dome on the roof, with a ring of lights.
    meshing.tube(
        Vec3::new(0.0, 5.6, 0.0),
        Vec3::new(0.0, 7.0, 0.0),
        1.7,
        1.7,
        12,
        stone,
        false,
    );
    let dome = (0..=6)
        .map(|k| {
            let a = k as f32 / 6.0 * FRAC_PI_2;
            (1.85 * a.cos(), 1.35 * a.sin())
        })
        .collect::<Vec<_>>();
    meshing.lathe(Vec3::Y * 7.0, &dome, 12, |_, _| pal.slate_roof);
    meshing.tube(
        Vec3::Y * 8.3,
        Vec3::Y * 9.0,
        0.07,
        0.02,
        6,
        pal.brass,
        true,
    );
    meshing.blob(
        Vec3::Y * 8.45,
        Vec3::splat(0.17),
        3,
        0.0,
        0.0,
        |_| pal.brass,
    );
    for k in 0..8 {
        let a = (k as f32 + 0.5) / 8.0 * TAU;
        glass.rbox(
            Vec3::new(a.sin() * 1.71, 6.5, a.cos() * 1.71),
            Vec3::new(0.26, 0.34, 0.03),
            0.04,
            Quat::from_rotation_y(a),
            Vec3::ONE,
            0.0,
        );
    }
    // The doors, with their knobs.
    for side in [-1.0f32, 1.0] {
        slab(
            &mut meshing,
            Vec3::new(side * 0.47, 2.1, 3.04),
            Vec3::new(0.45, 1.5, 0.07),
            pal.timber_dark,
        );
        meshing.blob(
            Vec3::new(side * 0.14, 2.0, 3.15),
            Vec3::splat(0.07),
            3,
            0.0,
            0.0,
            |_| pal.brass,
        );
    }
    slab(
        &mut meshing,
        Vec3::new(0.0, 3.72, 3.04),
        Vec3::new(1.1, 0.12, 0.12),
        shade,
    );
    // Windows full of books: beside the doors and along each side.
    let mut salt = 0;
    for x in [-2.75f32, 2.75] {
        window(
            &mut meshing,
            &mut glass,
            pal,
            (Vec3::new(x, 2.55, 3.02), Vec3::Z),
            salt,
        );
        salt += 1;
    }
    for side in [-1.0f32, 1.0] {
        for z in [-1.9f32, 0.0, 1.9] {
            window(
                &mut meshing,
                &mut glass,
                pal,
                (Vec3::new(side * 4.22, 2.55, z), Vec3::X * side),
                salt,
            );
            salt += 1;
        }
    }
    // A lamp on a post at each side of the steps.
    for side in [-1.0f32, 1.0] {
        let post = Vec3::new(side * 3.5, 0.0, 6.3);
        meshing.tube(
            post,
            post + Vec3::Y * 2.1,
            0.07,
            0.05,
            6,
            pal.steel,
            true,
        );
        slab(
            &mut meshing,
            post + Vec3::Y * 2.5,
            Vec3::new(0.2, 0.03, 0.2),
            pal.steel,
        );
        glass.rbox(
            post + Vec3::Y * 2.28,
            Vec3::new(0.14, 0.18, 0.14),
            0.03,
            Quat::IDENTITY,
            Vec3::ONE,
            0.0,
        );
    }
    // Books left in a pile on the porch.
    for (k, half) in [(0.3f32, 0.22f32), (0.26, 0.2), (0.22, 0.17)]
        .into_iter()
        .enumerate()
    {
        meshing.rbox(
            Vec3::new(-2.2, 0.66 + k as f32 * 0.1, 3.7),
            Vec3::new(half.0, 0.045, half.1),
            0.01,
            Quat::from_rotation_y(k as f32 * 0.5),
            pal.paints[(k * 2 + 1) % pal.paints.len()],
            0.0,
        );
    }
    // The owl that keeps watch, on the peak of the pediment.
    let owl = Vec3::new(0.0, ridge + 0.36, fore - 0.2);
    meshing.blob(owl, Vec3::new(0.3, 0.4, 0.27), 5, 0.0, 0.0, |_| {
        pal.leather
    });
    meshing.blob(
        owl + Vec3::new(0.0, -0.08, 0.1),
        Vec3::new(0.2, 0.27, 0.2),
        4,
        0.0,
        0.0,
        |_| pal.parchment,
    );
    for side in [-1.0f32, 1.0] {
        let eye = owl + Vec3::new(side * 0.12, 0.16, 0.2);
        meshing.blob(
            eye,
            Vec3::new(0.1, 0.1, 0.06),
            3,
            0.0,
            0.0,
            |_| pal.page,
        );
        meshing.blob(
            eye + Vec3::Z * 0.045,
            Vec3::new(0.045, 0.045, 0.03),
            3,
            0.0,
            0.0,
            |_| pal.ink,
        );
        meshing.tube(
            owl + Vec3::new(side * 0.16, 0.3, 0.02),
            owl + Vec3::new(side * 0.24, 0.5, 0.0),
            0.06,
            0.01,
            5,
            pal.leather,
            true,
        );
    }
    meshing.tube(
        owl + Vec3::new(0.0, 0.1, 0.24),
        owl + Vec3::new(0.0, 0.02, 0.34),
        0.045,
        0.005,
        5,
        pal.brass,
        true,
    );
    (meshing.build(), glass.build())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::{GROUNDS, SIZE};

    #[test]
    fn the_library_is_built_of_walls_and_of_glass() {
        let (walls, glass) = build(&Palette::default());
        for mesh in [walls, glass] {
            let points = mesh
                .attribute(Mesh::ATTRIBUTE_POSITION)
                .and_then(|points| points.as_float3())
                .unwrap();
            assert!(!points.is_empty());
            // It stands on the ground and within what it is
            // picked by.
            for [x, y, z] in points {
                assert!((-0.01..=SIZE.1).contains(y), "{y}");
                assert!(x.hypot(*z) < GROUNDS, "{x} {z}");
            }
        }
    }
}
