//! Tobias Cogsworth's stall of contraptions: a vault of brass and
//! steel with cogs that turn on its top, a boiler and its pipes, a
//! chimney, gadgets on the counter and a grindstone that throws
//! sparks.

use std::f32::consts::TAU;

use glam::{Affine3A, Quat, Vec2, Vec3};

use crate::build::mesh::Mb;
use crate::places::bazaar::build::stall::{
    DEEP, Dressed, Stand, WIDE, WORK, cog, turning,
};
use crate::places::bazaar::build::{Cog, FLOOR, Shapes, within};

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
pub const SPARKS: Vec3 =
    Vec3::new(STONE.0.x - STONE.1, STONE.0.y + 0.04, STONE.0.z);

/// The point of the vault at `a` radians from its top.
fn vault(a: f32) -> Vec2 {
    let (radius, middle) = ARC;
    Vec2::new(radius * a.sin(), middle + radius * a.cos())
}

/// The angle from the vault's top to where it meets the posts.
fn reach() -> f32 {
    ((WIDE + 0.2) / ARC.0).asin()
}

pub fn build(s: &mut Stand) -> Dressed {
    let (pal, style) = (s.pal, s.style);
    s.posts(pal.timber_dark, (POSTS, POSTS));
    s.backdrop(style.cloth, POSTS);
    // The vault, of slats of brass and steel from back to front, and
    // a rib of brass round each end.
    let (spring, _) = ARC;
    let reach = reach();
    let slats = 8;
    let (back, front) = (-DEEP - 0.2, DEEP + 0.5);
    for k in 0..slats {
        let a =
            -reach + 2.0 * reach * (k as f32 + 0.5) / slats as f32;
        let p = vault(a);
        s.mb.rbox(
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
    let ribs = 10;
    let rib =
        |k: usize| -reach + 2.0 * reach * k as f32 / ribs as f32;
    for z in [back, front] {
        for k in 0..ribs {
            let (p, q) = (vault(rib(k)), vault(rib(k + 1)));
            s.mb.rod(
                Vec3::new(p.x, p.y, z),
                Vec3::new(q.x, q.y, z),
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
    let big = (Vec3::new(0.3, crown + 0.72, 0.25), 0.5);
    let small = (big.0 + Vec3::new(-0.72, -0.22, 0.0), 0.28);
    for (at, _) in [big, small] {
        s.mb.tube(
            Vec3::new(at.x, crown - 0.05, 0.1),
            Vec3::new(at.x, at.y, 0.1),
            0.04,
            0.035,
            6,
            pal.timber_dark,
            true,
        );
        s.mb.rod(
            Vec3::new(at.x, at.y, 0.05),
            Vec3::new(at.x, at.y, at.z + 0.12),
            0.035,
            6,
            pal.steel,
            true,
        );
    }
    let teeth = (12, 7);
    let turns = 0.16;
    let mut cogs = vec![
        turning(big.0, (big.1, teeth.0), pal.brass, turns),
        turning(
            small.0,
            (small.1, teeth.1),
            style.trim,
            -turns * teeth.0 as f32 / teeth.1 as f32,
        ),
    ];
    boiler(s, crown);
    gadgets(s, &mut cogs);
    cogs.push(grindstone(s));
    // The sign: a cog of its trim.
    let (face, crest) = s.sign(
        Vec3::new(0.0, crown + 0.15, front + 0.05),
        crown - 0.05,
    );
    within(
        s.mb,
        Affine3A::from_translation(face + Vec3::Z * 0.02),
        |mb| cog(mb, 0.15, 9, style.trim),
    );
    s.finish(
        (
            Vec3::new(0.0, POSTS - 0.05, DEEP + 0.3),
            Vec3::new(-WIDE, POSTS, -DEEP),
        ),
        crest,
        [-1.0f32, 1.0].map(|side| {
            let p = vault(side * reach);
            Vec3::new(p.x, p.y - 0.02, front)
        }),
        cogs,
    )
}

/// A boiler at the back, its pipe up the post and over under the
/// vault, and a chimney through the vault's `crown`.
fn boiler(s: &mut Stand, crown: f32) {
    let (mb, pal, style) = (&mut *s.mb, s.pal, s.style);
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
    mb.rod(
        gauge,
        gauge + Vec3::Z * 0.03,
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
    mb.plate(
        gauge + Vec3::new(0.02, 0.02, 0.04),
        Vec3::new(0.006, 0.06, 0.004),
        Quat::from_rotation_z(-0.7),
        pal.ink,
    );
    let pipe = [
        boiler + Vec3::new(0.15, 1.05, 0.0),
        Vec3::new(-0.8, POSTS - 0.2, -DEEP + 0.32),
        Vec3::new(1.2, POSTS - 0.2, -DEEP + 0.32),
        Vec3::new(1.2, FLOOR + 1.3, -DEEP + 0.32),
        Vec3::new(0.7, FLOOR + 1.3, -DEEP + 0.32),
    ];
    for pair in pipe.windows(2) {
        mb.rod(pair[0], pair[1], 0.04, 6, pal.steel_bright, false);
    }
    // A collar at each bend.
    for at in pipe {
        mb.ball(at, Vec3::splat(0.06), 3, pal.brass);
    }
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
}

/// Gadgets on the counter: a bell jar with a spark in it, springs, a
/// clockwork beetle and a crank whose cog turns.
fn gadgets(s: &mut Stand, cogs: &mut Vec<Cog>) {
    let (pal, style) = (s.pal, s.style);
    let top = s.counter(pal.timber_dark);
    let mb = &mut *s.mb;
    let bell = top + Vec3::new(-0.6, 0.0, 0.0);
    mb.rod(bell, bell + Vec3::Y * 0.03, 0.15, 12, pal.timber, true);
    s.glass.lathe(
        bell + Vec3::Y * 0.03,
        &[(0.12, 0.0), (0.12, 0.22), (0.08, 0.3), (0.0, 0.32)],
        10,
        |_, _| Vec3::ONE,
    );
    s.gleam.ball(
        bell + Vec3::Y * 0.15,
        Vec3::splat(0.05),
        3,
        style.glow,
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
            mb.rod(
                beetle + Vec3::new(side * 0.05, -0.02, z),
                beetle + Vec3::new(side * 0.1, -0.05, z * 1.4),
                0.006,
                3,
                pal.steel,
                false,
            );
        }
    }
    mb.rod(
        beetle + Vec3::Y * 0.04,
        beetle + Vec3::new(0.0, 0.1, -0.05),
        0.01,
        4,
        pal.steel,
        false,
    );
    let crank = top + Vec3::new(0.65, 0.12, -0.05);
    mb.slab(crank, Vec3::new(0.15, 0.12, 0.12), style.cloth * 0.8);
    mb.rod(
        crank + Vec3::new(0.0, 0.0, 0.12),
        crank + Vec3::new(0.0, 0.0, 0.16),
        0.02,
        5,
        pal.steel,
        true,
    );
    cogs.push(turning(
        crank + Vec3::Z * 0.17,
        (0.12, 8),
        pal.brass,
        -0.4,
    ));
    mb.rod(
        crank + Vec3::new(0.0, 0.12, 0.0),
        crank + Vec3::new(0.0, 0.28, 0.0),
        0.012,
        4,
        pal.steel,
        false,
    );
    mb.ball(
        crank + Vec3::Y * 0.3,
        Vec3::splat(0.035),
        2,
        pal.bazaar.awnings[0],
    );
}

/// The grindstone on its frame, which turns while the tinker works
/// it. Returns the stone.
fn grindstone(s: &mut Stand) -> Cog {
    let (mb, pal) = (&mut *s.mb, s.pal);
    let (stone, radius) = STONE;
    for z in [-0.12f32, 0.12] {
        for x in [-0.2f32, 0.2] {
            mb.rod(
                Vec3::new(stone.x + x, FLOOR, stone.z + z * 1.5),
                Vec3::new(stone.x, stone.y, stone.z + z),
                0.03,
                4,
                pal.timber,
                true,
            );
        }
    }
    mb.rod(
        stone - Vec3::Z * 0.16,
        stone + Vec3::Z * 0.16,
        0.025,
        5,
        pal.steel,
        true,
    );
    let mut wheel = Mb::new();
    wheel.rod(
        Vec3::Z * -0.05,
        Vec3::Z * 0.05,
        radius,
        16,
        pal.rock,
        true,
    );
    for k in 0..4 {
        let a = k as f32 / 4.0 * TAU;
        wheel.plate(
            Vec3::new(a.cos(), a.sin(), 0.0) * radius * 0.5
                + Vec3::Z * 0.055,
            Vec3::new(radius * 0.4, 0.015, 0.005),
            Quat::from_rotation_z(a),
            pal.rock_dark,
        );
    }
    Cog {
        mesh: wheel,
        at: stone,
        lies: Quat::IDENTITY,
        rate: 1.1,
    }
}
