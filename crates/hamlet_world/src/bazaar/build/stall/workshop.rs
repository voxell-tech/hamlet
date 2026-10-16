//! Wren Halloway's workshop: a booth under a roof of shingles, a
//! bench of little robots half built, figurines on a shelf, tools on
//! the wall and the makers' banners at its posts.

use bevy::prelude::*;

use super::super::{FLOOR, cloth, lantern, slab};
use super::{
    DEEP, Dressed, HIGH, LOW, WIDE, WORK, backdrop, mast, posts, sign,
};
use crate::meshes::Mb;
use crate::palette::{Palette, StallStyle};

/// A little robot standing on `at`, `size` tall, painted `paint`:
/// a body, a round head with lit eyes and an antenna. `done` leaves
/// it whole, or with its head beside it and an arm missing.
fn robot(
    mb: &mut Mb,
    gleam: &mut Mb,
    pal: &Palette,
    (at, size): (Vec3, f32),
    paint: Vec3,
    (glow, done): (Vec3, bool),
) {
    let s = size;
    for side in [-1.0f32, 1.0] {
        mb.tube(
            at + Vec3::new(side * 0.1 * s, 0.0, 0.0),
            at + Vec3::new(side * 0.1 * s, 0.3 * s, 0.0),
            0.05 * s,
            0.05 * s,
            5,
            pal.steel,
            true,
        );
    }
    let body = at + Vec3::Y * 0.5 * s;
    mb.rbox(
        body,
        Vec3::new(0.2, 0.22, 0.15) * s,
        0.04 * s,
        Quat::IDENTITY,
        paint,
        0.1,
    );
    mb.rbox(
        body + Vec3::new(0.0, 0.02 * s, 0.15 * s),
        Vec3::new(0.11, 0.1, 0.01) * s,
        0.01 * s,
        Quat::IDENTITY,
        pal.brass,
        0.0,
    );
    let arms =
        [-1.0f32, 1.0].into_iter().take(if done { 2 } else { 1 });
    for side in arms {
        mb.tube(
            body + Vec3::new(side * 0.21 * s, 0.12 * s, 0.0),
            body + Vec3::new(side * 0.3 * s, -0.12 * s, 0.05 * s),
            0.04 * s,
            0.035 * s,
            5,
            pal.steel,
            true,
        );
    }
    // A head on its neck, or set down beside it to be fitted.
    let head = match done {
        true => body + Vec3::Y * 0.38 * s,
        false => at + Vec3::new(0.45 * s, 0.16 * s, 0.1 * s),
    };
    if done {
        mb.tube(
            body + Vec3::Y * 0.2 * s,
            head,
            0.04 * s,
            0.04 * s,
            5,
            pal.steel,
            false,
        );
    } else {
        // Its wires stand out of its neck.
        for (k, colour) in
            [pal.awnings[0], pal.awnings[1]].into_iter().enumerate()
        {
            let x = (k as f32 - 0.5) * 0.06 * s;
            mb.tube(
                body + Vec3::new(x, 0.2 * s, 0.0),
                body + Vec3::new(x * 2.0, 0.34 * s, 0.04 * s),
                0.012 * s,
                0.012 * s,
                3,
                colour,
                false,
            );
        }
    }
    mb.blob(
        head,
        Vec3::new(0.17, 0.15, 0.15) * s,
        4,
        0.0,
        0.0,
        |_| paint,
    );
    for side in [-1.0f32, 1.0] {
        gleam.blob(
            head + Vec3::new(side * 0.06 * s, 0.01 * s, 0.13 * s),
            Vec3::splat(0.03 * s),
            2,
            0.0,
            0.0,
            |_| glow,
        );
    }
    mb.tube(
        head + Vec3::Y * 0.13 * s,
        head + Vec3::Y * 0.26 * s,
        0.01 * s,
        0.01 * s,
        3,
        pal.steel,
        false,
    );
    gleam.blob(
        head + Vec3::Y * 0.28 * s,
        Vec3::splat(0.03 * s),
        2,
        0.0,
        0.0,
        |_| glow,
    );
}

/// A hammer, a saw, a wrench and a pair of pliers hung on the board
/// whose face is at `at`.
fn tools(mb: &mut Mb, pal: &Palette, at: Vec3) {
    let flat = |mb: &mut Mb,
                c: Vec3,
                half: Vec2,
                turn: f32,
                colour: Vec3| {
        mb.rbox(
            at + c,
            half.extend(0.012),
            0.006,
            Quat::from_rotation_z(turn),
            colour,
            0.0,
        );
    };
    // The hammer.
    flat(
        mb,
        Vec3::new(-0.75, 0.0, 0.0),
        Vec2::new(0.025, 0.2),
        0.0,
        pal.timber,
    );
    flat(
        mb,
        Vec3::new(-0.75, 0.2, 0.01),
        Vec2::new(0.09, 0.035),
        0.0,
        pal.steel,
    );
    // The saw.
    flat(
        mb,
        Vec3::new(-0.38, 0.05, 0.0),
        Vec2::new(0.05, 0.27),
        0.0,
        pal.steel_bright,
    );
    flat(
        mb,
        Vec3::new(-0.38, -0.25, 0.01),
        Vec2::new(0.06, 0.06),
        0.0,
        pal.timber,
    );
    // The wrench.
    flat(
        mb,
        Vec3::new(0.05, 0.0, 0.0),
        Vec2::new(0.022, 0.2),
        0.15,
        pal.steel,
    );
    mb.torus(
        at + Vec3::new(0.08, 0.2, 0.0),
        Vec3::Z,
        0.045,
        0.018,
        8,
        pal.steel,
    );
    // The pliers.
    for turn in [-0.15f32, 0.15] {
        flat(
            mb,
            Vec3::new(0.4, 0.0, 0.0),
            Vec2::new(0.018, 0.18),
            turn,
            pal.awnings[0],
        );
    }
    flat(
        mb,
        Vec3::new(0.4, 0.17, 0.01),
        Vec2::new(0.02, 0.06),
        0.0,
        pal.steel,
    );
    // A coil of wire and a ruler.
    mb.torus(
        at + Vec3::new(0.78, 0.08, 0.02),
        Vec3::Z,
        0.1,
        0.02,
        12,
        pal.brass,
    );
    flat(
        mb,
        Vec3::new(0.1, -0.32, 0.0),
        Vec2::new(0.4, 0.025),
        0.0,
        pal.ochre,
    );
}

/// The stall.
pub(super) fn build(
    mb: &mut Mb,
    (glass, gleam): (&mut Mb, &mut Mb),
    pal: &Palette,
    style: StallStyle,
) -> Dressed {
    posts(mb, pal.timber, (LOW, HIGH));
    backdrop(mb, pal.log, HIGH);
    // A roof of shingles, in rows from the back down over the front,
    // and a board along its front in the stall's colour.
    let (back, front) =
        ((-DEEP - 0.2, HIGH + 0.1), (DEEP + 0.45, LOW - 0.05));
    let rows = 6;
    let edge = WIDE + 0.3;
    for row in 0..rows {
        let t = |k: f32| k / rows as f32;
        let (z0, y0) = (
            back.0 + (front.0 - back.0) * t(row as f32),
            back.1 + (front.1 - back.1) * t(row as f32),
        );
        let (z1, y1) = (
            back.0 + (front.0 - back.0) * t(row as f32 + 1.25),
            back.1 + (front.1 - back.1) * t(row as f32 + 1.25),
        );
        let shade = [1.0, 0.86, 0.94][row % 3];
        let middle =
            Vec3::new(0.0, (y0 + y1) * 0.5 + 0.03, (z0 + z1) * 0.5);
        let slope = (y0 - y1).atan2(z1 - z0);
        mb.rbox(
            middle,
            Vec3::new(edge, 0.025, (z1 - z0).hypot(y0 - y1) * 0.5),
            0.01,
            Quat::from_rotation_x(slope),
            pal.timber_dark.lerp(pal.log, 0.4) * shade,
            0.2,
        );
    }
    slab(
        mb,
        Vec3::new(0.0, front.1 - 0.04, front.0 + 0.02),
        Vec3::new(edge, 0.09, 0.025),
        style.cloth,
    );
    slab(
        mb,
        Vec3::new(0.0, front.1 - 0.04, front.0 + 0.045),
        Vec3::new(edge, 0.015, 0.01),
        style.trim,
    );
    // Its tools hung on the back wall over a shelf of figurines.
    let wall = Vec3::new(0.0, FLOOR + 1.75, -DEEP + 0.01);
    slab(mb, wall, Vec3::new(1.15, 0.45, 0.02), pal.log * 1.1);
    tools(mb, pal, wall + Vec3::Z * 0.03);
    let shelf = Vec3::new(0.0, FLOOR + 1.05, -DEEP + 0.15);
    slab(mb, shelf, Vec3::new(1.2, 0.025, 0.14), pal.timber);
    for (k, x) in [-0.95f32, -0.6, -0.25, 0.1, 0.45, 0.8]
        .into_iter()
        .enumerate()
    {
        let paint = pal.paints[(k * 3 + 1) % pal.paints.len()];
        robot(
            mb,
            gleam,
            pal,
            (
                shelf + Vec3::new(x, 0.025, 0.0),
                0.32 + 0.06 * (k % 2) as f32,
            ),
            paint,
            (style.glow, true),
        );
    }
    // A workbench for a counter, its top of thick planks, with a vice
    // at one end and two robots on it: one half built, one done.
    let at = Vec3::new(0.0, FLOOR, 0.45);
    for x in [-0.85f32, 0.85] {
        for z in [-0.2f32, 0.2] {
            mb.tube(
                at + Vec3::new(x, 0.0, z),
                at + Vec3::new(x, 0.84, z),
                0.05,
                0.05,
                5,
                pal.timber,
                true,
            );
        }
    }
    slab(
        mb,
        at + Vec3::Y * 0.3,
        Vec3::new(0.88, 0.03, 0.24),
        pal.timber,
    );
    for k in 0..3 {
        slab(
            mb,
            at + Vec3::new(0.0, 0.87, -0.22 + 0.22 * k as f32),
            Vec3::new(1.05, 0.04, 0.105),
            pal.log * [1.0, 0.9, 1.05][k],
        );
    }
    // A banner across its front.
    mb.rbox(
        at + Vec3::new(0.0, 0.62, 0.3),
        Vec3::new(0.86, 0.14, 0.006),
        0.0,
        Quat::IDENTITY,
        style.cloth,
        0.0,
    );
    for k in 0..6 {
        mb.rbox(
            at + Vec3::new(-0.72 + 0.29 * k as f32, 0.5, 0.31),
            Vec3::new(0.06, 0.06, 0.006),
            0.0,
            Quat::from_rotation_z(std::f32::consts::FRAC_PI_4),
            style.trim,
            0.0,
        );
    }
    let top = at + Vec3::Y * 0.91;
    let vice = top + Vec3::new(-0.9, 0.0, 0.18);
    slab(
        mb,
        vice + Vec3::Y * 0.07,
        Vec3::new(0.06, 0.07, 0.1),
        pal.steel,
    );
    slab(
        mb,
        vice + Vec3::new(0.12, 0.07, 0.0),
        Vec3::new(0.03, 0.07, 0.1),
        pal.steel,
    );
    mb.tube(
        vice + Vec3::new(-0.08, 0.05, 0.0),
        vice + Vec3::new(-0.08, 0.05, 0.25),
        0.012,
        0.012,
        4,
        pal.steel_bright,
        true,
    );
    robot(
        mb,
        gleam,
        pal,
        (top + Vec3::new(-0.35, 0.0, 0.0), 0.5),
        pal.paints[2],
        (style.glow, false),
    );
    robot(
        mb,
        gleam,
        pal,
        (top + Vec3::new(0.45, 0.0, -0.05), 0.55),
        pal.paints[3],
        (style.glow, true),
    );
    // Gears and bolts strewn about them.
    for (k, (x, z)) in [(-0.6f32, 0.15f32), (0.1, 0.2), (0.85, 0.12)]
        .into_iter()
        .enumerate()
    {
        mb.torus(
            top + Vec3::new(x, 0.015, z),
            Vec3::Y,
            0.05 + 0.015 * k as f32,
            0.015,
            10,
            pal.brass,
        );
    }
    // The makers' banners, hung out from the front posts.
    for side in [-1.0f32, 1.0] {
        let x = side * (WIDE + 0.32);
        let hang = LOW - 0.15;
        mb.tube(
            Vec3::new(side * WIDE, hang, DEEP),
            Vec3::new(side * (WIDE + 0.55), hang, DEEP),
            0.02,
            0.02,
            4,
            pal.timber_dark,
            true,
        );
        let long = 1.15;
        cloth(
            mb,
            [
                Vec3::new(x - 0.2, hang, DEEP),
                Vec3::new(x + 0.2, hang, DEEP),
                Vec3::new(x + 0.2, hang - long, DEEP),
                Vec3::new(x - 0.2, hang - long, DEEP),
            ],
            Vec3::Z,
            style.cloth,
        );
        cloth(
            mb,
            [
                Vec3::new(x - 0.2, hang - long, DEEP),
                Vec3::new(x + 0.2, hang - long, DEEP),
                Vec3::new(x, hang - long - 0.2, DEEP),
                Vec3::new(x, hang - long - 0.2, DEEP),
            ],
            Vec3::Z,
            style.cloth,
        );
        for out in [1.0f32, -1.0] {
            let face = Vec3::new(x, hang - 0.45, DEEP + out * 0.008);
            // A hammer crossed with a spanner.
            for turn in [-0.6f32, 0.6] {
                mb.rbox(
                    face,
                    Vec3::new(0.022, 0.16, 0.004),
                    0.0,
                    Quat::from_rotation_z(turn),
                    style.trim,
                    0.0,
                );
            }
            mb.rbox(
                face + Vec3::new(0.0, -0.36, 0.0),
                Vec3::new(0.16, 0.025, 0.004),
                0.0,
                Quat::IDENTITY,
                style.trim,
                0.0,
            );
        }
    }
    // A stool with a robot as tall as a child, which Wren is
    // building, and a crate of parts on the other side.
    let stool = WORK;
    for k in 0..3 {
        let a = k as f32 / 3.0 * std::f32::consts::TAU;
        mb.tube(
            stool + Vec3::new(a.sin() * 0.16, 0.0, a.cos() * 0.16),
            stool + Vec3::new(a.sin() * 0.1, 0.4, a.cos() * 0.1),
            0.025,
            0.025,
            4,
            pal.timber,
            true,
        );
    }
    mb.tube(
        stool + Vec3::Y * 0.4,
        stool + Vec3::Y * 0.45,
        0.19,
        0.19,
        10,
        pal.log,
        true,
    );
    robot(
        mb,
        gleam,
        pal,
        (stool + Vec3::Y * 0.45, 0.85),
        pal.paints[0],
        (style.glow, true),
    );
    let crate_at = Vec3::new(-WIDE - 0.5, FLOOR + 0.18, 0.4);
    mb.rbox(
        crate_at,
        Vec3::new(0.26, 0.18, 0.22),
        0.02,
        Quat::from_rotation_y(0.3),
        pal.timber,
        0.2,
    );
    for k in 0..4 {
        let a = k as f32 * 1.7;
        mb.torus(
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
    let (face, crest) = sign(
        mb,
        pal,
        style,
        Vec3::new(0.0, LOW + 0.25, DEEP + 0.5),
        front.1 + 0.05,
    );
    mb.rbox(
        face,
        Vec3::new(0.14, 0.12, 0.03),
        0.03,
        Quat::IDENTITY,
        style.trim,
        0.0,
    );
    for side in [-1.0f32, 1.0] {
        gleam.blob(
            face + Vec3::new(side * 0.06, 0.02, 0.03),
            Vec3::splat(0.03),
            2,
            0.0,
            0.0,
            |_| style.glow,
        );
        mb.rbox(
            face + Vec3::new(side * 0.36, 0.0, 0.0),
            Vec3::new(0.12, 0.02, 0.012),
            0.0,
            Quat::from_rotation_z(side * 0.4),
            style.trim,
            0.0,
        );
    }
    mb.tube(
        face + Vec3::Y * 0.12,
        face + Vec3::Y * 0.2,
        0.01,
        0.01,
        3,
        style.trim,
        false,
    );
    let lamp = lantern(
        mb,
        glass,
        pal,
        Vec3::new(0.0, LOW - 0.05, DEEP + 0.2),
    );
    Dressed {
        lamp,
        mast: mast(mb, pal, Vec3::new(WIDE, HIGH, -DEEP)),
        crest,
        ties: [-1.0f32, 1.0].map(|side| {
            Vec3::new(side * edge, front.1 - 0.1, front.0 + 0.05)
        }),
        cogs: Vec::new(),
    }
}
