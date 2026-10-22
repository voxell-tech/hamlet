//! Madame Perpetua's costumier's booth: a rail of robes at the back,
//! a dress form, bolts of cloth and pots of paint on the counter, a
//! looking glass, and a little robot turning on a turntable.

use std::f32::consts::{FRAC_PI_2, TAU};

use glam::{Affine3A, Quat, Vec3};

use crate::build::mesh::Mb;
use crate::places::bazaar::build::stall::{
    DEEP, Dressed, HIGH, LOW, Stand, WIDE, WORK, awning_ends,
};
use crate::places::bazaar::build::{Cog, FLOOR, Shapes, within};
use crate::world::palette::{Palette, StallStyle};

/// The height of the turntable's plinth, and the turns a second of
/// what stands on it.
const PLINTH: f32 = 0.2;
const TURNS: f32 = 0.06;

/// A robe on a hanger from the rail at `at`, `long` from its
/// shoulders to its hem, turned `turn` about the rail.
fn robe(
    mb: &mut Mb,
    pal: &Palette,
    at: Vec3,
    long: f32,
    turn: f32,
    colour: Vec3,
) {
    let rot = Quat::from_rotation_y(turn);
    mb.torus(
        at + Vec3::Y * 0.03,
        rot * Vec3::Z,
        0.03,
        0.007,
        6,
        pal.brass,
    );
    mb.tube(
        at + rot * Vec3::new(-0.16, -0.06, 0.0),
        at + rot * Vec3::new(0.16, -0.06, 0.0),
        0.012,
        0.012,
        4,
        pal.timber,
        true,
    );
    mb.rbox(
        at + Vec3::Y * -0.1,
        Vec3::new(0.18, 0.05, 0.05),
        0.03,
        rot,
        colour,
        0.0,
    );
    mb.rbox(
        at + Vec3::Y * (-0.08 - long * 0.5),
        Vec3::new(0.15, long * 0.5, 0.04),
        0.02,
        rot,
        colour,
        0.15,
    );
    mb.rbox(
        at + Vec3::Y * (-0.06 - long),
        Vec3::new(0.16, 0.025, 0.045),
        0.01,
        rot,
        colour * 0.8,
        0.0,
    );
}

/// A dress form of `colour` standing on `at`.
fn dress_form(mb: &mut Mb, pal: &Palette, at: Vec3, colour: Vec3) {
    for k in 0..3 {
        let a = k as f32 / 3.0 * TAU + 0.4;
        mb.tube(
            at + Vec3::Y * 0.25,
            at + Vec3::new(a.sin() * 0.22, 0.0, a.cos() * 0.22),
            0.015,
            0.015,
            4,
            pal.timber_dark,
            false,
        );
    }
    mb.tube(
        at,
        at + Vec3::Y * 0.9,
        0.02,
        0.02,
        5,
        pal.timber_dark,
        false,
    );
    mb.lathe(
        at + Vec3::Y * 0.85,
        &[
            (0.0, 0.0),
            (0.14, 0.0),
            (0.18, 0.12),
            (0.13, 0.3),
            (0.17, 0.46),
            (0.16, 0.56),
            (0.06, 0.63),
            (0.035, 0.7),
            (0.0, 0.71),
        ],
        10,
        |ring, _| match ring {
            6.. => pal.timber_dark,
            _ => colour,
        },
    );
    // A tape measure hung about its neck.
    mb.torus(
        at + Vec3::Y * 1.46,
        Vec3::new(0.0, 1.0, 0.25),
        0.1,
        0.008,
        10,
        pal.ochre,
    );
}

/// A bolt of cloth of `colour` lying along X on `at`, `long` long.
fn bolt(mb: &mut Mb, at: Vec3, long: f32, colour: Vec3) {
    mb.tube(
        at - Vec3::X * long * 0.5,
        at + Vec3::X * long * 0.5,
        0.065,
        0.065,
        8,
        colour,
        true,
    );
}

/// A pot of paint of `colour` on `at`, its paint in `gleam` when it
/// is magic.
fn pot(
    mb: &mut Mb,
    gleam: &mut Mb,
    pal: &Palette,
    at: Vec3,
    (colour, magic): (Vec3, bool),
) {
    mb.lathe(
        at,
        &[(0.0, 0.0), (0.055, 0.0), (0.06, 0.1), (0.05, 0.1)],
        8,
        |_, _| pal.steel_bright,
    );
    let top = at + Vec3::Y * 0.1;
    let paint = match magic {
        true => gleam,
        false => mb,
    };
    paint.tube(
        top - Vec3::Y * 0.01,
        top,
        0.05,
        0.05,
        8,
        colour,
        true,
    );
}

/// A little robot of `paint` on a disc that turns about the axle
/// along its Z: built upright, then laid on its back, for the
/// turntable to stand up again.
fn turntable(pal: &Palette, style: StallStyle) -> Mb {
    let mut mb = Mb::new();
    let paint = pal.paints[3];
    within(&mut mb, Affine3A::from_rotation_x(FRAC_PI_2), |mb| {
        mb.tube(
            Vec3::ZERO,
            Vec3::Y * 0.04,
            0.3,
            0.3,
            14,
            style.trim,
            true,
        );
        for side in [-1.0f32, 1.0] {
            mb.tube(
                Vec3::new(side * 0.06, 0.04, 0.0),
                Vec3::new(side * 0.06, 0.2, 0.0),
                0.03,
                0.03,
                5,
                pal.steel,
                true,
            );
            mb.tube(
                Vec3::new(side * 0.15, 0.36, 0.0),
                Vec3::new(side * 0.19, 0.2, 0.03),
                0.025,
                0.025,
                5,
                pal.steel,
                true,
            );
        }
        mb.rbox(
            Vec3::Y * 0.31,
            Vec3::new(0.12, 0.12, 0.09),
            0.04,
            Quat::IDENTITY,
            paint,
            0.1,
        );
        mb.rbox(
            Vec3::Y * 0.52,
            Vec3::new(0.1, 0.08, 0.08),
            0.04,
            Quat::IDENTITY,
            paint,
            0.0,
        );
        mb.rbox(
            Vec3::new(0.0, 0.53, 0.075),
            Vec3::new(0.07, 0.045, 0.01),
            0.01,
            Quat::IDENTITY,
            pal.bed,
            0.0,
        );
        mb.tube(
            Vec3::Y * 0.6,
            Vec3::Y * 0.7,
            0.008,
            0.008,
            3,
            pal.brass,
            false,
        );
        mb.blob(
            Vec3::Y * 0.71,
            Vec3::splat(0.02),
            2,
            0.0,
            0.0,
            |_| style.glow,
        );
    });
    mb
}

pub fn build(s: &mut Stand) -> Dressed {
    let (pal, style) = (s.pal, s.style);
    s.posts(pal.timber_dark, (LOW, HIGH));
    s.rail(LOW - 0.08);
    s.awning([style.cloth, pal.whitewash]);
    s.backdrop(style.cloth * 0.7, HIGH);
    let top = s.counter(pal.timber);
    let (mb, gleam) = (&mut *s.mb, &mut *s.gleam);
    // A rail of robes along the back.
    let back = -DEEP + 0.28;
    let bar = FLOOR + 1.85;
    for side in [-1.0f32, 1.0] {
        mb.tube(
            Vec3::new(side * (WIDE - 0.2), FLOOR, back),
            Vec3::new(side * (WIDE - 0.2), bar + 0.05, back),
            0.025,
            0.025,
            5,
            pal.brass,
            true,
        );
    }
    mb.tube(
        Vec3::new(-(WIDE - 0.2), bar, back),
        Vec3::new(WIDE - 0.2, bar, back),
        0.018,
        0.018,
        5,
        pal.brass,
        true,
    );
    for k in 0..6 {
        let x = -0.95 + 1.9 * k as f32 / 5.0;
        robe(
            mb,
            pal,
            Vec3::new(x, bar, back),
            0.75 + 0.15 * ((k * 2) % 3) as f32,
            0.5 - 0.2 * (k % 3) as f32,
            pal.cloaks[(k * 3) % pal.cloaks.len()],
        );
    }
    dress_form(mb, pal, Vec3::new(-0.75, FLOOR, -0.1), style.cloth);
    // Bolts of cloth at one end of the counter, pots of paint at the
    // other, one of them magic.
    for (at, colour) in [
        (Vec3::new(-0.62, 0.065, -0.07), pal.bazaar.awnings[1]),
        (Vec3::new(-0.62, 0.065, 0.07), pal.bazaar.awnings[2]),
        (Vec3::new(-0.6, 0.18, 0.0), style.cloth),
    ] {
        bolt(mb, top + at, 0.55, colour);
    }
    for (k, x) in [0.25f32, 0.42, 0.6].into_iter().enumerate() {
        let colour = match k {
            1 => (style.glow, true),
            _ => (pal.paints[k * 2], false),
        };
        pot(
            mb,
            gleam,
            pal,
            top + Vec3::new(x, 0.0, 0.04 * k as f32),
            colour,
        );
    }
    mb.tube(
        top + Vec3::new(0.62, 0.08, 0.08),
        top + Vec3::new(0.72, 0.3, 0.12),
        0.01,
        0.01,
        4,
        pal.timber,
        true,
    );
    // A round looking glass on a stand by the front.
    let glass_at = Vec3::new(-WIDE - 0.4, FLOOR, DEEP + 0.1);
    for side in [-1.0f32, 1.0] {
        mb.tube(
            glass_at + Vec3::new(side * 0.36, 0.0, 0.0),
            glass_at + Vec3::new(side * 0.36, 1.05, 0.0),
            0.022,
            0.022,
            4,
            pal.timber_dark,
            true,
        );
    }
    let tilt = Affine3A::from_rotation_translation(
        Quat::from_rotation_y(0.5) * Quat::from_rotation_x(-0.12),
        glass_at + Vec3::Y * 1.0,
    );
    within(mb, tilt, |mb| {
        mb.torus(Vec3::ZERO, Vec3::Z, 0.32, 0.025, 16, pal.brass);
        mb.tube(
            Vec3::Z * -0.01,
            Vec3::Z * 0.01,
            0.31,
            0.31,
            16,
            pal.steel_bright,
            true,
        );
    });
    // The turntable at Madame Perpetua's side, on a plinth.
    mb.lathe(
        WORK,
        &[
            (0.0, 0.0),
            (0.3, 0.0),
            (0.28, PLINTH * 0.8),
            (0.32, PLINTH),
            (0.0, PLINTH),
        ],
        12,
        |_, _| pal.timber_dark,
    );
    let cogs = vec![Cog {
        mesh: turntable(pal, style),
        at: WORK + Vec3::Y * PLINTH,
        lies: Quat::from_rotation_x(-FRAC_PI_2),
        rate: TURNS,
        phase: 0.0,
    }];
    // The sign: a spool of thread with a needle through it.
    let (face, crest) =
        s.sign(Vec3::new(0.0, LOW + 0.18, DEEP + 0.55), LOW - 0.12);
    let mb = &mut *s.mb;
    within(
        mb,
        Affine3A::from_rotation_translation(
            Quat::from_rotation_z(FRAC_PI_2),
            face + Vec3::new(0.12, 0.0, 0.07),
        ),
        |mb| {
            mb.lathe(
                Vec3::ZERO,
                &[
                    (0.0, 0.0),
                    (0.09, 0.0),
                    (0.09, 0.03),
                    (0.06, 0.03),
                    (0.06, 0.21),
                    (0.09, 0.21),
                    (0.09, 0.24),
                    (0.0, 0.24),
                ],
                10,
                |ring, _| match ring {
                    3 | 4 => style.glow,
                    _ => style.trim,
                },
            );
        },
    );
    mb.tube(
        face + Vec3::new(-0.3, -0.12, 0.08),
        face + Vec3::new(0.05, 0.14, 0.08),
        0.008,
        0.004,
        4,
        pal.steel_bright,
        true,
    );
    mb.slab(
        face + Vec3::new(-0.3, -0.12, 0.08),
        Vec3::new(0.012, 0.012, 0.004),
        pal.steel_bright,
    );
    s.finish(
        (
            Vec3::new(0.0, LOW - 0.08, DEEP + 0.25),
            Vec3::new(WIDE, HIGH, -DEEP),
        ),
        crest,
        awning_ends(),
        cogs,
    )
}
