//! Tobias Cogsworth's stall of contraptions: a vault of brass and
//! steel with cogs that turn on its top, a boiler and its pipes, a
//! chimney, gadgets on the counter and a grindstone that throws
//! sparks.

use std::f32::consts::TAU;

use bevy::math::Affine3A;
use bevy::prelude::*;

use super::super::{FLOOR, lantern, slab};
use super::{
    DEEP, Dressed, Gear, WIDE, WORK, backdrop, cog, counter, mast,
    posts, sign, turning, within,
};
use crate::meshes::Mb;
use crate::palette::{Palette, StallStyle};

/// The height of the posts, where the vault springs from them.
const POSTS: f32 = 2.5;

/// The circle the vault is cut from, its radius and the height of
/// its middle: it springs from just over the posts and rises 0.55
/// over them.
const ARC: (f32, f32) = (2.75, 0.35);

/// The grindstone's middle, from the stall's middle, and its radius.
const STONE: (Vec3, f32) =
    (Vec3::new(WORK.x, FLOOR + 0.78, WORK.z), 0.3);

/// The spot on the grindstone the sparks fly from: its rim, on the
/// side the tinker stands.
pub(in crate::bazaar) const SPARKS: Vec3 =
    Vec3::new(STONE.0.x - STONE.1, STONE.0.y + 0.04, STONE.0.z);

/// The point of the vault at `a` radians from its top.
fn vault(a: f32) -> Vec2 {
    let (radius, middle) = ARC;
    Vec2::new(radius * a.sin(), middle + radius * a.cos())
}

/// A pipe through `points`, with a collar at each bend.
fn pipe(mb: &mut Mb, pal: &Palette, points: &[Vec3], colour: Vec3) {
    for pair in points.windows(2) {
        mb.tube(pair[0], pair[1], 0.04, 0.04, 6, colour, false);
    }
    for at in points {
        mb.blob(*at, Vec3::splat(0.06), 3, 0.0, 0.0, |_| pal.brass);
    }
}

/// The stall.
pub(super) fn build(
    mb: &mut Mb,
    (glass, gleam): (&mut Mb, &mut Mb),
    pal: &Palette,
    style: StallStyle,
) -> Dressed {
    posts(mb, pal.timber_dark, (POSTS, POSTS));
    backdrop(mb, style.cloth, POSTS);
    // The vault, of slats of brass and steel from back to front, and
    // a rib of brass round each end.
    let (spring, _) = ARC;
    let half = (WIDE + 0.2) / spring;
    let reach = half.asin();
    let slats = 8;
    let (back, front) = (-DEEP - 0.2, DEEP + 0.5);
    for k in 0..slats {
        let a =
            -reach + 2.0 * reach * (k as f32 + 0.5) / slats as f32;
        let p = vault(a);
        mb.rbox(
            Vec3::new(p.x, p.y, (back + front) * 0.5),
            Vec3::new(
                reach * spring / slats as f32 + 0.01,
                0.03,
                (front - back) * 0.5,
            ),
            0.01,
            Quat::from_rotation_z(-a),
            [pal.brass, pal.steel][k % 2],
            0.15,
        );
    }
    for z in [back, front] {
        let ribs = 10;
        for k in 0..ribs {
            let (a, b) = (
                -reach + 2.0 * reach * k as f32 / ribs as f32,
                -reach + 2.0 * reach * (k + 1) as f32 / ribs as f32,
            );
            let (p, q) = (vault(a), vault(b));
            mb.tube(
                Vec3::new(p.x, p.y, z),
                Vec3::new(q.x, q.y, z),
                0.035,
                0.035,
                5,
                pal.brass,
                false,
            );
        }
    }
    let crown = vault(0.0).y + 0.03;
    // The contraption on its top: two cogs on a frame, turning
    // together, a big one and a small one.
    let gear = Gear::sized(0.5, 12);
    let pinion = gear.mate(7);
    let big = Vec3::new(0.3, crown + 0.72, 0.25);
    let small = gear.beside(pinion, big, Vec2::new(-0.72, -0.22));
    for at in [big, small] {
        mb.tube(
            Vec3::new(at.x, crown - 0.05, 0.1),
            Vec3::new(at.x, at.y, 0.1),
            0.04,
            0.035,
            6,
            pal.timber_dark,
            true,
        );
        mb.tube(
            Vec3::new(at.x, at.y, 0.05),
            Vec3::new(at.x, at.y, at.z + 0.12),
            0.035,
            0.035,
            6,
            pal.steel,
            true,
        );
    }
    let turns = 0.16;
    let mut cogs = vec![
        turning((big, Quat::IDENTITY), gear, pal.brass, (turns, 0.0)),
        turning(
            (small, Quat::IDENTITY),
            pinion,
            style.trim,
            (
                gear.driven(pinion, turns),
                gear.phase_of(pinion, (big, 0.0), small),
            ),
        ),
    ];
    // A boiler at the back, its pipe up the post and over under the
    // vault, and a chimney through it.
    let boiler = Vec3::new(-0.95, FLOOR, -DEEP + 0.32);
    mb.lathe(
        boiler,
        &[
            (0.0, 0.0),
            (0.28, 0.0),
            (0.3, 0.08),
            (0.3, 0.95),
            (0.24, 1.08),
            (0.12, 1.14),
            (0.0, 1.15),
        ],
        12,
        |ring, _| match ring {
            1 | 4 => pal.brass * 0.85,
            _ => pal.brass,
        },
    );
    for y in [0.25f32, 0.6, 0.9] {
        mb.torus(
            boiler + Vec3::Y * y,
            Vec3::Y,
            0.305,
            0.018,
            14,
            style.trim,
        );
    }
    // A round gauge on the boiler's face.
    let gauge = boiler + Vec3::new(0.0, 0.72, 0.3);
    mb.tube(
        gauge,
        gauge + Vec3::Z * 0.03,
        0.09,
        0.09,
        12,
        pal.whitewash,
        true,
    );
    mb.torus(
        gauge + Vec3::Z * 0.03,
        Vec3::Z,
        0.09,
        0.012,
        12,
        pal.brass,
    );
    mb.rbox(
        gauge + Vec3::new(0.02, 0.02, 0.04),
        Vec3::new(0.006, 0.06, 0.004),
        0.0,
        Quat::from_rotation_z(-0.7),
        pal.ink,
        0.0,
    );
    pipe(
        mb,
        pal,
        &[
            boiler + Vec3::new(0.15, 1.05, 0.0),
            Vec3::new(-0.8, POSTS - 0.2, -DEEP + 0.32),
            Vec3::new(1.2, POSTS - 0.2, -DEEP + 0.32),
            Vec3::new(1.2, FLOOR + 1.3, -DEEP + 0.32),
            Vec3::new(0.7, FLOOR + 1.3, -DEEP + 0.32),
        ],
        pal.steel_bright,
    );
    let chimney = Vec3::new(-0.95, 0.0, -DEEP + 0.32);
    mb.tube(
        chimney + Vec3::Y * (FLOOR + 1.1),
        chimney + Vec3::Y * (crown + 0.85),
        0.07,
        0.06,
        8,
        pal.steel,
        false,
    );
    mb.lathe(
        chimney + Vec3::Y * (crown + 0.82),
        &[(0.06, 0.0), (0.16, 0.12), (0.16, 0.16), (0.0, 0.2)],
        10,
        |_, _| pal.brass,
    );
    // Gadgets on the counter: a bell jar with a spark in it, springs,
    // a clockwork beetle and a crank whose cog turns.
    let top = counter(mb, pal, style, pal.timber_dark);
    let bell = top + Vec3::new(-0.6, 0.0, 0.0);
    mb.tube(
        bell,
        bell + Vec3::Y * 0.03,
        0.15,
        0.15,
        12,
        pal.timber,
        true,
    );
    glass.lathe(
        bell + Vec3::Y * 0.03,
        &[(0.12, 0.0), (0.12, 0.22), (0.08, 0.3), (0.0, 0.32)],
        10,
        |_, _| Vec3::ONE,
    );
    gleam.blob(
        bell + Vec3::Y * 0.15,
        Vec3::splat(0.05),
        3,
        0.0,
        0.0,
        |_| style.glow,
    );
    for k in 0..3 {
        let at = top + Vec3::new(-0.25 + 0.1 * k as f32, 0.0, 0.15);
        for turn in 0..5 {
            mb.torus(
                at + Vec3::Y * (0.02 + 0.025 * turn as f32),
                Vec3::Y,
                0.035,
                0.008,
                8,
                pal.steel_bright,
            );
        }
    }
    let beetle = top + Vec3::new(0.15, 0.05, 0.05);
    mb.blob(beetle, Vec3::new(0.07, 0.05, 0.1), 4, 0.0, 0.0, |d| {
        match d.x.abs() < 0.12 {
            true => pal.brass * 0.7,
            false => pal.brass,
        }
    });
    for side in [-1.0f32, 1.0] {
        for z in [-0.05f32, 0.0, 0.05] {
            mb.tube(
                beetle + Vec3::new(side * 0.05, -0.02, z),
                beetle + Vec3::new(side * 0.1, -0.05, z * 1.4),
                0.006,
                0.006,
                3,
                pal.steel,
                false,
            );
        }
    }
    mb.tube(
        beetle + Vec3::Y * 0.04,
        beetle + Vec3::new(0.0, 0.1, -0.05),
        0.01,
        0.01,
        4,
        pal.steel,
        false,
    );
    let crank = top + Vec3::new(0.65, 0.12, -0.05);
    slab(mb, crank, Vec3::new(0.15, 0.12, 0.12), style.cloth * 0.8);
    mb.tube(
        crank + Vec3::new(0.0, 0.0, 0.12),
        crank + Vec3::new(0.0, 0.0, 0.16),
        0.02,
        0.02,
        5,
        pal.steel,
        true,
    );
    cogs.push(turning(
        (crank + Vec3::Z * 0.17, Quat::IDENTITY),
        Gear::sized(0.12, 8),
        pal.brass,
        (-0.4, 0.0),
    ));
    mb.tube(
        crank + Vec3::new(0.0, 0.12, 0.0),
        crank + Vec3::new(0.0, 0.28, 0.0),
        0.012,
        0.012,
        4,
        pal.steel,
        false,
    );
    mb.blob(
        crank + Vec3::Y * 0.3,
        Vec3::splat(0.035),
        2,
        0.0,
        0.0,
        |_| pal.awnings[0],
    );
    // The grindstone on its frame, which turns while the tinker
    // works it.
    let (stone, radius) = STONE;
    for z in [-0.12f32, 0.12] {
        mb.tube(
            Vec3::new(stone.x - 0.2, FLOOR, stone.z + z * 1.5),
            Vec3::new(stone.x, stone.y, stone.z + z),
            0.03,
            0.03,
            4,
            pal.timber,
            true,
        );
        mb.tube(
            Vec3::new(stone.x + 0.2, FLOOR, stone.z + z * 1.5),
            Vec3::new(stone.x, stone.y, stone.z + z),
            0.03,
            0.03,
            4,
            pal.timber,
            true,
        );
    }
    mb.tube(
        stone - Vec3::Z * 0.16,
        stone + Vec3::Z * 0.16,
        0.025,
        0.025,
        5,
        pal.steel,
        true,
    );
    let mut wheel = Mb::new();
    wheel.tube(
        Vec3::Z * -0.05,
        Vec3::Z * 0.05,
        radius,
        radius,
        16,
        pal.rock,
        true,
    );
    for k in 0..4 {
        let a = k as f32 / 4.0 * TAU;
        wheel.rbox(
            Vec3::new(a.cos(), a.sin(), 0.0) * radius * 0.5
                + Vec3::Z * 0.055,
            Vec3::new(radius * 0.4, 0.015, 0.005),
            0.0,
            Quat::from_rotation_z(a),
            pal.rock_dark,
            0.0,
        );
    }
    cogs.push(super::super::Cog {
        mesh: wheel.build(),
        at: stone,
        lies: Quat::IDENTITY,
        rate: 1.1,
        phase: 0.0,
    });
    // The sign: a cog of its trim.
    let (face, crest) = sign(
        mb,
        pal,
        style,
        Vec3::new(0.0, crown + 0.15, front + 0.05),
        crown - 0.05,
    );
    within(
        mb,
        Affine3A::from_translation(face + Vec3::Z * 0.02),
        |mb| {
            cog(mb, Gear::sized(0.15, 9), style.trim);
        },
    );
    let lamp = lantern(
        mb,
        glass,
        pal,
        Vec3::new(0.0, POSTS - 0.05, DEEP + 0.3),
    );
    Dressed {
        lamp,
        mast: mast(mb, pal, Vec3::new(-WIDE, POSTS, -DEEP)),
        crest,
        ties: [-1.0f32, 1.0].map(|side| {
            let p = vault(side * reach);
            Vec3::new(p.x, p.y - 0.02, front)
        }),
        cogs,
    }
}
