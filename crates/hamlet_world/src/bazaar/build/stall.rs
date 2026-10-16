//! A stall of the bazaar: posts, a striped awning, a counter, goods
//! hanging from a rail and what it sells.

use bevy::prelude::*;

use super::{FLOOR, cloth, lantern, slab, stripe};
use crate::meshes::Mb;
use crate::palette::Palette;

/// The height of a stall's front posts and of its back ones.
pub(super) const LOW: f32 = 2.25;
pub(super) const HIGH: f32 = 2.75;

/// What a stall sells.
#[derive(Clone, Copy, Debug)]
pub(super) enum Wares {
    Pots,
    Rugs,
    Spices,
    Scrolls,
}

/// What a stall sells on its counter, whose top is at `top`.
fn on_counter(
    mb: &mut Mb,
    pal: &Palette,
    top: Vec3,
    (colour, wares): (usize, Wares),
) {
    match wares {
        Wares::Pots => {
            for (k, x) in
                [-0.7f32, -0.3, 0.12, 0.55].into_iter().enumerate()
            {
                let size = 0.12 + 0.03 * (k % 2) as f32;
                let colour = [pal.terracotta, pal.ochre][k % 2];
                let at = top + Vec3::new(x, size, 0.0);
                mb.blob(
                    at,
                    Vec3::new(size, size * 1.2, size),
                    4,
                    0.0,
                    0.0,
                    |_| colour,
                );
                mb.torus(
                    at + Vec3::Y * size * 1.1,
                    Vec3::Y,
                    size * 0.55,
                    0.02,
                    10,
                    pal.brass,
                );
            }
            // Great jars on the paving before it.
            for (at, size) in [
                (Vec3::new(0.6, 0.0, 1.05), 0.3),
                (Vec3::new(-0.55, 0.0, 1.15), 0.22),
            ] {
                mb.blob(
                    at + Vec3::Y * (FLOOR + size),
                    Vec3::new(size * 0.85, size, size * 0.85),
                    5,
                    0.0,
                    0.0,
                    |_| pal.terracotta,
                );
                mb.torus(
                    at + Vec3::Y * (FLOOR + size * 1.9),
                    Vec3::Y,
                    size * 0.4,
                    0.03,
                    10,
                    pal.terracotta * 0.8,
                );
            }
        }
        Wares::Rugs => {
            for k in 0..3 {
                let y = 0.08 + 0.15 * k as f32;
                let z = 0.06 - 0.12 * (k % 2) as f32;
                mb.tube(
                    top + Vec3::new(-0.8, y, z),
                    top + Vec3::new(0.8, y, z),
                    0.075,
                    0.075,
                    8,
                    pal.awnings[(colour + k + 1) % pal.awnings.len()],
                    true,
                );
            }
            // One unrolled over the front of the counter.
            for (half, z, rug) in [
                (
                    Vec2::new(0.6, 0.38),
                    0.79,
                    pal.awnings[(colour + 3) % 4],
                ),
                (Vec2::new(0.42, 0.24), 0.805, pal.whitewash),
                (
                    Vec2::new(0.18, 0.12),
                    0.82,
                    pal.awnings[colour % 4],
                ),
            ] {
                mb.rbox(
                    Vec3::new(0.0, FLOOR + 0.5, z),
                    half.extend(0.012),
                    0.005,
                    Quat::IDENTITY,
                    rug,
                    0.0,
                );
            }
        }
        Wares::Spices => {
            // Baskets heaped with spices in cones, two rows of them.
            let spices = [
                pal.terracotta,
                pal.awnings[1],
                pal.moss,
                pal.ochre,
                pal.leather,
                pal.awnings[0],
            ];
            for (k, spice) in spices.into_iter().enumerate() {
                let at = top
                    + Vec3::new(
                        -0.68 + 0.27 * (k % 6) as f32,
                        0.0,
                        if k % 2 == 0 { 0.08 } else { -0.1 },
                    );
                mb.lathe(
                    at,
                    &[
                        (0.0, 0.0),
                        (0.11, 0.0),
                        (0.14, 0.08),
                        (0.15, 0.12),
                    ],
                    10,
                    |_, _| pal.log,
                );
                mb.lathe(
                    at + Vec3::Y * 0.1,
                    &[(0.14, 0.0), (0.0, 0.2)],
                    10,
                    |_, _| spice,
                );
            }
            // A crate of fruit on the paving beside it.
            let crate_at = Vec3::new(-0.6, FLOOR + 0.16, 1.05);
            mb.rbox(
                crate_at,
                Vec3::new(0.3, 0.16, 0.22),
                0.02,
                Quat::from_rotation_y(0.2),
                pal.timber,
                0.2,
            );
            for i in 0..6 {
                let at = crate_at
                    + Vec3::new(
                        -0.18 + 0.18 * (i % 3) as f32,
                        0.18 + 0.03 * (i / 3) as f32,
                        -0.07 + 0.14 * (i / 3) as f32,
                    );
                mb.blob(at, Vec3::splat(0.075), 3, 0.0, 0.0, |_| {
                    pal.flowers[3]
                });
            }
        }
        Wares::Scrolls => {
            for k in 0..4 {
                let (y, z) = (
                    0.05 + 0.04 * (k % 2) as f32,
                    -0.1 + 0.07 * k as f32,
                );
                mb.tube(
                    top + Vec3::new(-0.75, y, z),
                    top + Vec3::new(-0.15, y, z),
                    0.04,
                    0.04,
                    6,
                    pal.parchment,
                    true,
                );
            }
            for k in 0..4 {
                let half = 0.17 - 0.015 * k as f32;
                mb.rbox(
                    top + Vec3::new(
                        0.45,
                        0.03 + 0.06 * k as f32,
                        0.0,
                    ),
                    Vec3::new(half, 0.028, half * 0.75),
                    0.008,
                    Quat::from_rotation_y(0.25 * k as f32),
                    pal.paints[(k * 2 + colour) % pal.paints.len()],
                    0.0,
                );
            }
            // A globe on a stand.
            let globe = top + Vec3::new(0.05, 0.0, 0.1);
            mb.tube(
                globe,
                globe + Vec3::Y * 0.12,
                0.05,
                0.02,
                6,
                pal.brass,
                true,
            );
            mb.blob(
                globe + Vec3::Y * 0.24,
                Vec3::splat(0.12),
                5,
                0.0,
                0.0,
                |d| match d.y.abs() < 0.3 || d.x > 0.5 {
                    true => pal.moss,
                    false => pal.water,
                },
            );
            mb.torus(
                globe + Vec3::Y * 0.24,
                Vec3::X,
                0.14,
                0.01,
                12,
                pal.brass,
            );
        }
    }
}

/// What hangs from the rail under a stall's awning.
fn hanging(
    mb: &mut Mb,
    pal: &Palette,
    (colour, wares): (usize, Wares),
) {
    let rail = |x: f32| Vec3::new(x, LOW - 0.08, 0.75);
    let chain = |mb: &mut Mb, from: Vec3, to: Vec3| {
        mb.tube(from, to, 0.008, 0.008, 3, pal.steel, false);
    };
    match wares {
        Wares::Pots => {
            for (k, x) in [-0.6f32, 0.0, 0.55].into_iter().enumerate()
            {
                let drop = 0.35 + 0.12 * (k % 2) as f32;
                let at = rail(x) - Vec3::Y * drop;
                chain(mb, rail(x), at + Vec3::Y * 0.1);
                mb.blob(
                    at,
                    Vec3::new(0.13, 0.11, 0.13),
                    4,
                    0.0,
                    0.0,
                    |d| match d.y > 0.4 {
                        true => pal.brass * 1.2,
                        false => pal.brass,
                    },
                );
                mb.torus(
                    at + Vec3::Y * 0.09,
                    Vec3::Y,
                    0.1,
                    0.015,
                    10,
                    pal.brass,
                );
            }
        }
        Wares::Rugs => {
            for (k, x) in [-0.45f32, 0.45].into_iter().enumerate() {
                let at = rail(x) - Vec3::Y * 0.42;
                mb.rbox(
                    at,
                    Vec3::new(0.3, 0.38, 0.012),
                    0.005,
                    Quat::IDENTITY,
                    pal.awnings[(colour + k + 2) % pal.awnings.len()],
                    0.0,
                );
                mb.rbox(
                    at + Vec3::Z * 0.008,
                    Vec3::new(0.22, 0.3, 0.012),
                    0.005,
                    Quat::IDENTITY,
                    pal.whitewash,
                    0.0,
                );
                mb.rbox(
                    at + Vec3::Z * 0.016,
                    Vec3::new(0.1, 0.1, 0.012),
                    0.005,
                    Quat::from_rotation_z(
                        std::f32::consts::FRAC_PI_4,
                    ),
                    pal.awnings[(colour + k) % pal.awnings.len()],
                    0.0,
                );
            }
        }
        Wares::Spices => {
            // Strings of chillies and of garlic.
            for (k, x) in
                [-0.7f32, -0.35, 0.35, 0.7].into_iter().enumerate()
            {
                let (hue, size) = match k % 2 {
                    0 => (
                        pal.awnings[0],
                        Vec3::new(0.035, 0.07, 0.035),
                    ),
                    _ => (pal.whitewash, Vec3::splat(0.055)),
                };
                chain(mb, rail(x), rail(x) - Vec3::Y * 0.12);
                for i in 0..6 {
                    let at = rail(x)
                        - Vec3::Y * (0.18 + 0.09 * i as f32)
                        + Vec3::X * 0.02 * ((i % 2) as f32 - 0.5);
                    mb.blob(at, size, 3, 0.0, 0.0, |_| hue);
                }
            }
        }
        Wares::Scrolls => {
            for (k, x) in [-0.5f32, 0.5].into_iter().enumerate() {
                let at = rail(x) - Vec3::Y * 0.35;
                mb.tube(
                    rail(x) - Vec3::new(0.22, 0.03, 0.0),
                    rail(x) + Vec3::new(0.22, -0.03, 0.0),
                    0.02,
                    0.02,
                    5,
                    pal.timber_dark,
                    true,
                );
                mb.rbox(
                    at,
                    Vec3::new(0.18, 0.3, 0.008),
                    0.004,
                    Quat::IDENTITY,
                    pal.parchment,
                    0.0,
                );
                for line in 0..4 {
                    mb.rbox(
                        at + Vec3::new(
                            0.0,
                            0.15 - 0.1 * line as f32,
                            0.009,
                        ),
                        Vec3::new(
                            0.12 - 0.02 * ((line + k) % 3) as f32,
                            0.012,
                            0.004,
                        ),
                        0.0,
                        Quat::IDENTITY,
                        pal.ink,
                        0.0,
                    );
                }
            }
        }
    }
}

/// One stall, built facing +Z on its own spot: four posts and a mast,
/// a striped awning, a counter, what hangs from its rail and what it
/// sells. Returns the spot of its lantern.
pub(super) fn stall(
    mb: &mut Mb,
    glass: &mut Mb,
    pal: &Palette,
    (colour, sells): (usize, Wares),
) -> Vec3 {
    for (x, z, tall) in [
        (-1.0f32, 0.75f32, LOW),
        (1.0, 0.75, LOW),
        (-1.0, -0.75, HIGH),
        (1.0, -0.75, HIGH),
    ] {
        mb.tube(
            Vec3::new(x, FLOOR, z),
            Vec3::new(x, tall + 0.05, z),
            0.06,
            0.05,
            6,
            pal.timber,
            true,
        );
    }
    // A mast for its flag over one back post, and a rail between
    // the front ones.
    mb.tube(
        Vec3::new(1.0, HIGH, -0.75),
        Vec3::new(1.0, HIGH + 0.65, -0.75),
        0.03,
        0.02,
        5,
        pal.timber_dark,
        true,
    );
    mb.blob(
        Vec3::new(1.0, HIGH + 0.67, -0.75),
        Vec3::splat(0.04),
        2,
        0.0,
        0.0,
        |_| pal.brass,
    );
    mb.tube(
        Vec3::new(-1.0, LOW - 0.08, 0.75),
        Vec3::new(1.0, LOW - 0.08, 0.75),
        0.025,
        0.025,
        5,
        pal.timber_dark,
        true,
    );
    // The awning runs out over the front, past the posts, and ends
    // in a flap to each stripe, edged in brass.
    let (back, front) = ((-0.95, HIGH + 0.05), (1.25, LOW - 0.1));
    let strips = 7;
    for i in 0..strips {
        let (x0, x1) = (
            -1.25 + 2.5 * i as f32 / strips as f32,
            -1.25 + 2.5 * (i + 1) as f32 / strips as f32,
        );
        let colour = stripe(pal, colour, i);
        cloth(
            mb,
            [
                Vec3::new(x0, back.1, back.0),
                Vec3::new(x1, back.1, back.0),
                Vec3::new(x1, front.1, front.0),
                Vec3::new(x0, front.1, front.0),
            ],
            Vec3::new(0.0, 1.0, 0.45),
            colour,
        );
        let tip = Vec3::new(
            (x0 + x1) * 0.5,
            front.1 - 0.24,
            front.0 + 0.01,
        );
        cloth(
            mb,
            [
                Vec3::new(x0, front.1, front.0),
                Vec3::new(x1, front.1, front.0),
                tip,
                tip,
            ],
            Vec3::Z,
            colour,
        );
        mb.blob(tip, Vec3::splat(0.025), 2, 0.0, 0.0, |_| pal.brass);
    }
    cloth(
        mb,
        [
            Vec3::new(-1.0, FLOOR, -0.78),
            Vec3::new(1.0, FLOOR, -0.78),
            Vec3::new(1.0, HIGH, -0.78),
            Vec3::new(-1.0, HIGH, -0.78),
        ],
        Vec3::Z,
        pal.awnings[(colour + 2) % pal.awnings.len()],
    );
    slab(
        mb,
        Vec3::new(0.0, FLOOR + 0.42, 0.5),
        Vec3::new(0.92, 0.42, 0.28),
        pal.timber,
    );
    slab(
        mb,
        Vec3::new(0.0, FLOOR + 0.87, 0.5),
        Vec3::new(1.02, 0.03, 0.34),
        pal.timber_dark,
    );
    // A band of the stall's colour along the counter's front.
    mb.rbox(
        Vec3::new(0.0, FLOOR + 0.62, 0.785),
        Vec3::new(0.9, 0.08, 0.006),
        0.0,
        Quat::IDENTITY,
        pal.awnings[colour % pal.awnings.len()],
        0.0,
    );
    on_counter(
        mb,
        pal,
        Vec3::new(0.0, FLOOR + 0.9, 0.5),
        (colour, sells),
    );
    hanging(mb, pal, (colour, sells));
    // The lantern hangs from the awning before the rail.
    lantern(mb, glass, pal, Vec3::new(0.0, LOW - 0.04, 1.0))
}
