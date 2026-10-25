//! Wren Halloway's workshop: a booth under a roof of shingles, a
//! bench of little robots half built, figurines on a shelf, tools on
//! the wall and the makers' banners at its posts.

use std::f32::consts::{FRAC_PI_4, TAU};

use bevy::prelude::*;

use super::super::{FLOOR, Shapes};
use super::{DEEP, Dressed, HIGH, LOW, Stand, WIDE, WORK};
use crate::meshes::Meshing;
use crate::palette::Palette;

/// A little robot standing on `at`, `size` tall, painted `paint`:
/// a body, a round head with lit eyes and an antenna. `done` leaves
/// it whole, or with its head beside it and an arm missing.
fn robot(
    s: &mut Stand,
    (at, size): (Vec3, f32),
    paint: Vec3,
    done: bool,
) {
    let (mb, pal, glow) = (&mut *s.mb, s.pal, s.style.glow);
    let s3 = |v: Vec3| v * size;
    for side in [-1.0f32, 1.0] {
        mb.rod(
            at + s3(Vec3::new(side * 0.1, 0.0, 0.0)),
            at + s3(Vec3::new(side * 0.1, 0.3, 0.0)),
            0.05 * size,
            5,
            pal.steel,
            true,
        );
    }
    let body = at + Vec3::Y * 0.5 * size;
    mb.rbox(
        body,
        s3(Vec3::new(0.2, 0.22, 0.15)),
        0.04 * size,
        Quat::IDENTITY,
        paint,
        0.1,
    );
    mb.rbox(
        body + s3(Vec3::new(0.0, 0.02, 0.15)),
        s3(Vec3::new(0.11, 0.1, 0.01)),
        0.01 * size,
        Quat::IDENTITY,
        pal.brass,
        0.0,
    );
    let arms = if done { 2 } else { 1 };
    for side in [-1.0f32, 1.0].into_iter().take(arms) {
        mb.tube(
            body + s3(Vec3::new(side * 0.21, 0.12, 0.0)),
            body + s3(Vec3::new(side * 0.3, -0.12, 0.05)),
            0.04 * size,
            0.035 * size,
            5,
            pal.steel,
            true,
        );
    }
    // A head on its neck, or set down beside it to be fitted.
    let head = match done {
        true => body + Vec3::Y * 0.38 * size,
        false => at + s3(Vec3::new(0.45, 0.16, 0.1)),
    };
    if done {
        mb.rod(
            body + Vec3::Y * 0.2 * size,
            head,
            0.04 * size,
            5,
            pal.steel,
            false,
        );
    } else {
        // Its wires stand out of its neck.
        for (k, colour) in
            [pal.awnings[0], pal.awnings[1]].into_iter().enumerate()
        {
            let x = (k as f32 - 0.5) * 0.06 * size;
            mb.rod(
                body + Vec3::new(x, 0.2 * size, 0.0),
                body + Vec3::new(x * 2.0, 0.34 * size, 0.04 * size),
                0.012 * size,
                3,
                colour,
                false,
            );
        }
    }
    mb.ball(head, s3(Vec3::new(0.17, 0.15, 0.15)), 4, paint);
    for side in [-1.0f32, 1.0] {
        s.gleam.ball(
            head + s3(Vec3::new(side * 0.06, 0.01, 0.13)),
            Vec3::splat(0.03 * size),
            2,
            glow,
        );
    }
    mb.rod(
        head + Vec3::Y * 0.13 * size,
        head + Vec3::Y * 0.26 * size,
        0.01 * size,
        3,
        pal.steel,
        false,
    );
    s.gleam.ball(
        head + Vec3::Y * 0.28 * size,
        Vec3::splat(0.03 * size),
        2,
        glow,
    );
}

/// A hammer, a saw, a wrench and a pair of pliers hung on the board
/// whose face is at `at`, with a coil of wire and a ruler.
fn tools(mb: &mut Meshing, pal: &Palette, at: Vec3) {
    let mut flat = |c: Vec3, half: Vec2, turn: f32, colour: Vec3| {
        mb.rbox(
            at + c,
            half.extend(0.012),
            0.006,
            Quat::from_rotation_z(turn),
            colour,
            0.0,
        );
    };
    // The hammer, the saw and the wrench's handle.
    for (c, half, turn, colour) in [
        (
            Vec3::new(-0.75, 0.0, 0.0),
            Vec2::new(0.025, 0.2),
            0.0,
            pal.timber,
        ),
        (
            Vec3::new(-0.75, 0.2, 0.01),
            Vec2::new(0.09, 0.035),
            0.0,
            pal.steel,
        ),
        (
            Vec3::new(-0.38, 0.05, 0.0),
            Vec2::new(0.05, 0.27),
            0.0,
            pal.steel_bright,
        ),
        (
            Vec3::new(-0.38, -0.25, 0.01),
            Vec2::new(0.06, 0.06),
            0.0,
            pal.timber,
        ),
        (
            Vec3::new(0.05, 0.0, 0.0),
            Vec2::new(0.022, 0.2),
            0.15,
            pal.steel,
        ),
    ] {
        flat(c, half, turn, colour);
    }
    // The pliers.
    for turn in [-0.15f32, 0.15] {
        flat(
            Vec3::new(0.4, 0.0, 0.0),
            Vec2::new(0.018, 0.18),
            turn,
            pal.awnings[0],
        );
    }
    flat(
        Vec3::new(0.4, 0.17, 0.01),
        Vec2::new(0.02, 0.06),
        0.0,
        pal.steel,
    );
    flat(
        Vec3::new(0.1, -0.32, 0.0),
        Vec2::new(0.4, 0.025),
        0.0,
        pal.ochre,
    );
    mb.torus(
        at + Vec3::new(0.08, 0.2, 0.0),
        Vec3::Z,
        0.045,
        0.018,
        8,
        pal.steel,
    );
    mb.torus(
        at + Vec3::new(0.78, 0.08, 0.02),
        Vec3::Z,
        0.1,
        0.02,
        12,
        pal.brass,
    );
}

pub(super) fn build(s: &mut Stand) -> Dressed {
    let (pal, style) = (s.pal, s.style);
    s.posts(pal.timber, (LOW, HIGH));
    s.backdrop(pal.log, HIGH);
    // A roof of shingles, in rows from the back down over the front,
    // and a board along its front in the stall's colour.
    let (back, front) =
        ((-DEEP - 0.2, HIGH + 0.1), (DEEP + 0.45, LOW - 0.05));
    let rows = 6;
    let edge = WIDE + 0.3;
    let at = |t: f32| {
        let t = t / rows as f32;
        (
            back.0 + (front.0 - back.0) * t,
            back.1 + (front.1 - back.1) * t,
        )
    };
    for row in 0..rows {
        let ((z0, y0), (z1, y1)) =
            (at(row as f32), at(row as f32 + 1.25));
        let shade = [1.0, 0.86, 0.94][row % 3];
        let middle =
            Vec3::new(0.0, (y0 + y1) * 0.5 + 0.03, (z0 + z1) * 0.5);
        s.mb.rbox(
            middle,
            Vec3::new(edge, 0.025, (z1 - z0).hypot(y0 - y1) * 0.5),
            0.01,
            Quat::from_rotation_x((y0 - y1).atan2(z1 - z0)),
            pal.timber_dark.lerp(pal.log, 0.4) * shade,
            0.2,
        );
    }
    s.mb.slab(
        Vec3::new(0.0, front.1 - 0.04, front.0 + 0.02),
        Vec3::new(edge, 0.09, 0.025),
        style.cloth,
    );
    s.mb.slab(
        Vec3::new(0.0, front.1 - 0.04, front.0 + 0.045),
        Vec3::new(edge, 0.015, 0.01),
        style.trim,
    );
    // Its tools hung on the back wall over a shelf of figurines.
    let wall = Vec3::new(0.0, FLOOR + 1.75, -DEEP + 0.01);
    s.mb.slab(wall, Vec3::new(1.15, 0.45, 0.02), pal.log * 1.1);
    tools(s.mb, pal, wall + Vec3::Z * 0.03);
    let shelf = Vec3::new(0.0, FLOOR + 1.05, -DEEP + 0.15);
    s.mb.slab(shelf, Vec3::new(1.2, 0.025, 0.14), pal.timber);
    for (k, x) in [-0.95f32, -0.6, -0.25, 0.1, 0.45, 0.8]
        .into_iter()
        .enumerate()
    {
        robot(
            s,
            (
                shelf + Vec3::new(x, 0.025, 0.0),
                0.32 + 0.06 * (k % 2) as f32,
            ),
            pal.paints[(k * 3 + 1) % pal.paints.len()],
            true,
        );
    }
    bench(s);
    banners(s);
    // A stool with a robot as tall as a child, which Wren is
    // building, and a crate of parts on the other side.
    let stool = WORK;
    for k in 0..3 {
        let a = k as f32 / 3.0 * TAU;
        s.mb.rod(
            stool + Vec3::new(a.sin() * 0.16, 0.0, a.cos() * 0.16),
            stool + Vec3::new(a.sin() * 0.1, 0.4, a.cos() * 0.1),
            0.025,
            4,
            pal.timber,
            true,
        );
    }
    s.mb.rod(
        stool + Vec3::Y * 0.4,
        stool + Vec3::Y * 0.45,
        0.19,
        10,
        pal.log,
        true,
    );
    robot(s, (stool + Vec3::Y * 0.45, 0.85), pal.paints[0], true);
    let crate_at = Vec3::new(-WIDE - 0.5, FLOOR + 0.18, 0.4);
    s.mb.rbox(
        crate_at,
        Vec3::new(0.26, 0.18, 0.22),
        0.02,
        Quat::from_rotation_y(0.3),
        pal.timber,
        0.2,
    );
    for k in 0..4 {
        let a = k as f32 * 1.7;
        s.mb.torus(
            crate_at
                + Vec3::new(
                    a.sin() * 0.1,
                    0.19 + 0.02 * k as f32,
                    a.cos() * 0.08,
                ),
            Vec3::new(0.2 * a.cos(), 1.0, 0.2),
            0.06,
            0.018,
            8,
            [pal.brass, pal.steel][k % 2],
        );
    }
    // The sign: a robot's head with lit eyes.
    let (face, crest) = s
        .sign(Vec3::new(0.0, LOW + 0.25, DEEP + 0.5), front.1 + 0.05);
    s.mb.rbox(
        face,
        Vec3::new(0.14, 0.12, 0.03),
        0.03,
        Quat::IDENTITY,
        style.trim,
        0.0,
    );
    for side in [-1.0f32, 1.0] {
        s.gleam.ball(
            face + Vec3::new(side * 0.06, 0.02, 0.03),
            Vec3::splat(0.03),
            2,
            style.glow,
        );
        s.mb.plate(
            face + Vec3::new(side * 0.36, 0.0, 0.0),
            Vec3::new(0.12, 0.02, 0.012),
            Quat::from_rotation_z(side * 0.4),
            style.trim,
        );
    }
    s.mb.rod(
        face + Vec3::Y * 0.12,
        face + Vec3::Y * 0.2,
        0.01,
        3,
        style.trim,
        false,
    );
    s.finish(
        (
            Vec3::new(0.0, LOW - 0.05, DEEP + 0.2),
            Vec3::new(WIDE, HIGH, -DEEP),
        ),
        crest,
        [-1.0f32, 1.0].map(|side| {
            Vec3::new(side * edge, front.1 - 0.1, front.0 + 0.05)
        }),
        Vec::new(),
    )
}

/// A workbench for a counter, its top of thick planks, with a vice at
/// one end and two robots on it: one half built, one done.
fn bench(s: &mut Stand) {
    let (pal, style) = (s.pal, s.style);
    let at = Vec3::new(0.0, FLOOR, 0.45);
    for x in [-0.85f32, 0.85] {
        for z in [-0.2f32, 0.2] {
            s.mb.rod(
                at + Vec3::new(x, 0.0, z),
                at + Vec3::new(x, 0.84, z),
                0.05,
                5,
                pal.timber,
                true,
            );
        }
    }
    s.mb.slab(
        at + Vec3::Y * 0.3,
        Vec3::new(0.88, 0.03, 0.24),
        pal.timber,
    );
    for k in 0..3 {
        s.mb.slab(
            at + Vec3::new(0.0, 0.87, -0.22 + 0.22 * k as f32),
            Vec3::new(1.05, 0.04, 0.105),
            pal.log * [1.0, 0.9, 1.05][k],
        );
    }
    // A banner across its front.
    s.mb.plate(
        at + Vec3::new(0.0, 0.62, 0.3),
        Vec3::new(0.86, 0.14, 0.006),
        Quat::IDENTITY,
        style.cloth,
    );
    for k in 0..6 {
        s.mb.plate(
            at + Vec3::new(-0.72 + 0.29 * k as f32, 0.5, 0.31),
            Vec3::new(0.06, 0.06, 0.006),
            Quat::from_rotation_z(FRAC_PI_4),
            style.trim,
        );
    }
    let top = at + Vec3::Y * 0.91;
    let vice = top + Vec3::new(-0.9, 0.0, 0.18);
    s.mb.slab(
        vice + Vec3::Y * 0.07,
        Vec3::new(0.06, 0.07, 0.1),
        pal.steel,
    );
    s.mb.slab(
        vice + Vec3::new(0.12, 0.07, 0.0),
        Vec3::new(0.03, 0.07, 0.1),
        pal.steel,
    );
    s.mb.rod(
        vice + Vec3::new(-0.08, 0.05, 0.0),
        vice + Vec3::new(-0.08, 0.05, 0.25),
        0.012,
        4,
        pal.steel_bright,
        true,
    );
    robot(
        s,
        (top + Vec3::new(-0.35, 0.0, 0.0), 0.5),
        pal.paints[2],
        false,
    );
    robot(
        s,
        (top + Vec3::new(0.45, 0.0, -0.05), 0.55),
        pal.paints[3],
        true,
    );
    // Gears and bolts strewn about them.
    for (k, (x, z)) in [(-0.6f32, 0.15f32), (0.1, 0.2), (0.85, 0.12)]
        .into_iter()
        .enumerate()
    {
        s.mb.torus(
            top + Vec3::new(x, 0.015, z),
            Vec3::Y,
            0.05 + 0.015 * k as f32,
            0.015,
            10,
            pal.brass,
        );
    }
}

/// The makers' banners, hung out from the front posts.
fn banners(s: &mut Stand) {
    let (mb, pal, style) = (&mut *s.mb, s.pal, s.style);
    for side in [-1.0f32, 1.0] {
        let x = side * (WIDE + 0.32);
        let hang = LOW - 0.15;
        mb.rod(
            Vec3::new(side * WIDE, hang, DEEP),
            Vec3::new(side * (WIDE + 0.55), hang, DEEP),
            0.02,
            4,
            pal.timber_dark,
            true,
        );
        let foot = hang - 1.15;
        mb.cloth(
            [
                Vec3::new(x - 0.2, hang, DEEP),
                Vec3::new(x + 0.2, hang, DEEP),
                Vec3::new(x + 0.2, foot, DEEP),
                Vec3::new(x - 0.2, foot, DEEP),
            ],
            Vec3::Z,
            style.cloth,
        );
        mb.flap(
            (
                Vec3::new(x - 0.2, foot, DEEP),
                Vec3::new(x + 0.2, foot, DEEP),
            ),
            Vec3::new(x, foot - 0.2, DEEP),
            Vec3::Z,
            style.cloth,
        );
        for out in [1.0f32, -1.0] {
            let face = Vec3::new(x, hang - 0.45, DEEP + out * 0.008);
            // A hammer crossed with a spanner.
            for turn in [-0.6f32, 0.6] {
                mb.plate(
                    face,
                    Vec3::new(0.022, 0.16, 0.004),
                    Quat::from_rotation_z(turn),
                    style.trim,
                );
            }
            mb.plate(
                face + Vec3::new(0.0, -0.36, 0.0),
                Vec3::new(0.16, 0.025, 0.004),
                Quat::IDENTITY,
                style.trim,
            );
        }
    }
}
