//! The meshes of a mascot, which follow its look.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bevy::math::Affine3A;
use bevy::prelude::*;

use crate::chief::look::{Hat, Holds, Look};
use crate::meshes::Meshing;
use crate::palette::Palette;

/// The meshes of a mascot, each about its own joint.
pub struct Parts {
    pub cloak: Meshing,
    pub head: Meshing,
    pub eyes: Meshing,
    pub hat: Meshing,
    pub hand: Meshing,
    pub foot: Meshing,
    pub item: Meshing,
    pub letter: Meshing,
}

impl Parts {
    pub fn each(self) -> [Meshing; 8] {
        [
            self.cloak,
            self.head,
            self.eyes,
            self.hat,
            self.hand,
            self.foot,
            self.item,
            self.letter,
        ]
    }
}

fn ball(meshing: &mut Meshing, c: Vec3, r: Vec3, col: Vec3) {
    meshing.blob(c, r, 5, 0.0, 0.0, |_| col);
}

/// The hair of a head that wears no hat over it.
fn hair(meshing: &mut Meshing, pal: &Palette) {
    ball(
        meshing,
        Vec3::new(0.0, 0.1, -0.03),
        Vec3::new(0.278, 0.2, 0.272),
        pal.hair,
    );
}

fn hat(pal: &Palette, look: Look) -> Meshing {
    let mut meshing = Meshing::new();
    let cloth = look.cloak.colour(pal);
    let trim = pal.mantle.lerp(cloth, 0.25);
    match look.hat {
        Hat::Pointed => {
            // Tipped well back, so the face shows from above.
            meshing.at(Affine3A::from_rotation_x(-0.22));
            let felt = cloth * 0.8;
            let base = Vec3::Y * 0.15;
            meshing.lathe(
                base,
                &[
                    (0.0, 0.0),
                    (0.43, 0.0),
                    (0.43, 0.035),
                    (0.27, 0.08),
                ],
                14,
                |r, _| if r == 0 { felt * 0.7 } else { felt },
            );
            meshing.lathe(
                base,
                &[
                    (0.27, 0.08),
                    (0.21, 0.3),
                    (0.12, 0.54),
                    (0.06, 0.7),
                ],
                12,
                |_, _| felt,
            );
            meshing.torus(
                base + Vec3::Y * 0.11,
                Vec3::Y,
                0.262,
                0.03,
                14,
                trim,
            );
            let tip = base + Vec3::new(0.14, 0.88, -0.07);
            meshing.tube(
                base + Vec3::Y * 0.68,
                tip,
                0.065,
                0.03,
                8,
                felt,
                true,
            );
            ball(&mut meshing, tip, Vec3::splat(0.06), pal.brass);
            meshing.at(Affine3A::IDENTITY);
        }
        Hat::Hood => {
            // A lathe about the way the face looks, open in front.
            meshing.at(Affine3A::from_rotation_x(FRAC_PI_2));
            meshing.lathe(
                Vec3::ZERO,
                &[
                    (0.0, -0.33),
                    (0.18, -0.31),
                    (0.3, -0.16),
                    (0.335, 0.02),
                    (0.31, 0.15),
                    (0.27, 0.21),
                ],
                14,
                |_, _| cloth,
            );
            meshing.at(Affine3A::IDENTITY);
            meshing.torus(
                Vec3::Z * 0.21,
                Vec3::Z,
                0.272,
                0.028,
                14,
                trim,
            );
            meshing.tube(
                Vec3::new(0.0, 0.26, -0.12),
                Vec3::new(0.0, 0.5, -0.36),
                0.13,
                0.02,
                8,
                cloth,
                true,
            );
        }
        Hat::Crown => {
            hair(&mut meshing, pal);
            let ring = Vec3::Y * 0.24;
            meshing.torus(ring, Vec3::Y, 0.19, 0.035, 14, pal.brass);
            for i in 0..5 {
                let a = i as f32 / 5.0 * TAU;
                let foot =
                    ring + Vec3::new(a.sin(), 0.0, a.cos()) * 0.19;
                meshing.tube(
                    foot,
                    foot + Vec3::Y * 0.14,
                    0.045,
                    0.006,
                    6,
                    pal.brass,
                    true,
                );
                ball(
                    &mut meshing,
                    foot + Vec3::Y * 0.15,
                    Vec3::splat(0.022),
                    pal.wax,
                );
            }
        }
        Hat::Wreath => {
            hair(&mut meshing, pal);
            let ring = Vec3::new(0.0, 0.2, -0.02);
            let axis = Vec3::new(0.0, 1.0, 0.22);
            meshing.torus(
                ring,
                axis,
                0.235,
                0.05,
                14,
                pal.foliage_shade,
            );
            let (u, w) = axis.normalize().any_orthonormal_pair();
            for i in 0..6 {
                let a = i as f32 / 6.0 * TAU + 0.4;
                let at = ring
                    + (u * a.cos() + w * a.sin()) * 0.25
                    + Vec3::Y * 0.03;
                ball(
                    &mut meshing,
                    at,
                    Vec3::splat(0.05),
                    pal.flowers[i % pal.flowers.len()],
                );
            }
        }
        Hat::Bare => {
            hair(&mut meshing, pal);
            meshing.tube(
                Vec3::new(0.0, 0.27, 0.0),
                Vec3::new(0.09, 0.4, 0.03),
                0.045,
                0.014,
                6,
                pal.hair,
                true,
            );
        }
    }
    meshing
}

fn item(pal: &Palette, holds: Holds) -> Meshing {
    let mut meshing = Meshing::new();
    match holds {
        Holds::Quill => {
            let tip = Vec3::new(0.02, 0.38, -0.05);
            meshing.tube(
                Vec3::Y * -0.06,
                tip,
                0.012,
                0.006,
                5,
                pal.parchment,
                false,
            );
            meshing.blob(
                Vec3::new(0.014, 0.26, -0.035),
                Vec3::new(0.014, 0.14, 0.07),
                5,
                0.1,
                3.0,
                |d| {
                    pal.autumn
                        .lerp(pal.whitewash, (d.y * 0.6).max(0.0))
                },
            );
            meshing.tube(
                Vec3::Y * -0.11,
                Vec3::Y * -0.06,
                0.003,
                0.012,
                5,
                pal.ink,
                true,
            );
        }
        Holds::Teacup => {
            let at = Vec3::new(0.0, 0.03, 0.07);
            meshing.lathe(
                at,
                &[
                    (0.0, 0.0),
                    (0.12, 0.0),
                    (0.13, 0.016),
                    (0.0, 0.016),
                ],
                10,
                |_, _| pal.whitewash,
            );
            meshing.lathe(
                at,
                &[
                    (0.045, 0.016),
                    (0.086, 0.105),
                    (0.072, 0.105),
                    (0.072, 0.09),
                    (0.0, 0.09),
                ],
                10,
                |r, _| {
                    if r >= 3 {
                        pal.autumn_shade
                    } else {
                        pal.whitewash
                    }
                },
            );
            meshing.torus(
                at + Vec3::new(0.105, 0.062, 0.0),
                Vec3::Z,
                0.03,
                0.01,
                8,
                pal.whitewash,
            );
        }
        Holds::Book => {
            let at = Vec3::new(0.0, 0.03, 0.07);
            let tilt = Quat::from_rotation_x(-0.25);
            meshing.rbox(
                at,
                Vec3::new(0.11, 0.145, 0.04),
                0.012,
                tilt,
                pal.leather,
                0.1,
            );
            meshing.rbox(
                at + tilt * Vec3::new(0.014, 0.0, 0.0),
                Vec3::new(0.105, 0.13, 0.028),
                0.006,
                tilt,
                pal.page,
                0.0,
            );
            meshing.rbox(
                at + tilt * Vec3::new(0.0, 0.0, 0.042),
                Vec3::new(0.035, 0.035, 0.004),
                0.002,
                tilt * Quat::from_rotation_z(PI / 4.0),
                pal.brass,
                0.0,
            );
        }
        Holds::Nothing => {}
    }
    meshing
}

/// The meshes of a mascot that looks like `look`.
pub fn parts(pal: &Palette, look: Look) -> Parts {
    let cloth = look.cloak.colour(pal);
    let trim = pal.mantle.lerp(cloth, 0.25);

    let mut cloak = Meshing::new();
    cloak.lathe(
        Vec3::ZERO,
        &[
            (0.0, 0.09),
            (0.41, 0.09),
            (0.45, 0.14),
            (0.44, 0.2),
            (0.39, 0.36),
            (0.3, 0.58),
            (0.23, 0.74),
            (0.18, 0.83),
            (0.0, 0.86),
        ],
        14,
        |r, _| match r {
            0 | 1 => cloth * 0.7,
            2 => trim,
            _ => cloth * (0.84 + 0.04 * (r - 3) as f32),
        },
    );
    cloak.torus(Vec3::Y * 0.84, Vec3::Y, 0.17, 0.04, 12, trim);
    ball(
        &mut cloak,
        Vec3::new(0.0, 0.81, 0.21),
        Vec3::splat(0.045),
        pal.brass,
    );

    let mut head = Meshing::new();
    head.blob(
        Vec3::ZERO,
        Vec3::new(0.27, 0.26, 0.26),
        7,
        0.0,
        0.0,
        |_| pal.face,
    );
    for side in [-1.0f32, 1.0] {
        ball(
            &mut head,
            Vec3::new(side * 0.155, -0.055, 0.2),
            Vec3::new(0.055, 0.04, 0.03),
            pal.blush,
        );
    }
    ball(
        &mut head,
        Vec3::new(0.0, -0.095, 0.25),
        Vec3::new(0.032, 0.013, 0.012),
        pal.leather,
    );

    let mut eyes = Meshing::new();
    for side in [-1.0f32, 1.0] {
        ball(
            &mut eyes,
            Vec3::new(side * 0.095, 0.0, 0.245),
            Vec3::new(0.036, 0.058, 0.022),
            pal.ink,
        );
    }

    let mut hand = Meshing::new();
    ball(&mut hand, Vec3::ZERO, Vec3::splat(0.075), pal.face);
    let mut foot = Meshing::new();
    ball(
        &mut foot,
        Vec3::new(0.0, 0.065, 0.03),
        Vec3::new(0.1, 0.065, 0.15),
        pal.timber_dark,
    );

    let mut letter = Meshing::new();
    letter.rbox(
        Vec3::ZERO,
        Vec3::new(0.13, 0.09, 0.006),
        0.004,
        Quat::IDENTITY,
        pal.parchment,
        0.0,
    );
    ball(
        &mut letter,
        Vec3::new(0.0, -0.02, -0.008),
        Vec3::new(0.022, 0.022, 0.008),
        pal.wax,
    );

    Parts {
        cloak,
        head,
        eyes,
        hat: hat(pal, look),
        hand,
        foot,
        item: item(pal, look.holds),
        letter,
    }
}

/// The wick: a small flame, drawn in whatever colour its material
/// has.
pub fn wick() -> Meshing {
    let mut meshing = Meshing::new();
    meshing.lathe(
        Vec3::ZERO,
        &[
            (0.0, -0.075),
            (0.06, -0.04),
            (0.078, 0.015),
            (0.052, 0.09),
            (0.018, 0.17),
            (0.0, 0.21),
        ],
        10,
        |_, _| Vec3::ONE,
    );
    meshing
}
