//! What each keeper wears on its head and holds for its trade, over a
//! mascot's own parts.

use glam::{Affine3A, Quat, Vec3};

use crate::build::mesh::Mb;
use crate::places::bazaar::Stall;
use crate::places::bazaar::build::{Shapes, within};
use crate::world::palette::Palette;

/// The meshes a keeper wears: about its head, in its hand, and what
/// glows of it, in its hand.
pub struct Worn {
    pub head: Mb,
    pub held: Mb,
    pub glows: Mb,
}

/// What the keeper of `stall` wears.
pub fn worn(pal: &Palette, stall: Stall) -> Worn {
    let style = pal.bazaar.stalls[stall as usize];
    let (mut head, mut held, mut glows) =
        (Mb::new(), Mb::new(), Mb::new());
    // A feather that is white at its root and of the trim at its tip.
    let feather = |d: Vec3| {
        pal.whitewash
            .lerp(style.trim, (0.5 - d.y * 0.5).clamp(0.0, 1.0))
    };
    match stall {
        Stall::Models => {
            // A tall cap of its stall's cloth with a band and a
            // tassel, a pointed beard, and an orb of thought in its
            // hand.
            head.lathe(
                Vec3::Y * 0.17,
                &[(0.0, 0.0), (0.22, 0.0), (0.19, 0.34), (0.0, 0.35)],
                12,
                |_, _| style.cloth,
            );
            head.torus(
                Vec3::Y * 0.21,
                Vec3::Y,
                0.21,
                0.025,
                12,
                style.trim,
            );
            head.rod(
                Vec3::Y * 0.52,
                Vec3::new(0.16, 0.4, -0.08),
                0.01,
                3,
                style.trim,
                false,
            );
            head.ball(
                Vec3::new(0.17, 0.35, -0.09),
                Vec3::new(0.03, 0.06, 0.03),
                3,
                style.trim,
            );
            head.blob(
                Vec3::new(0.0, -0.24, 0.17),
                Vec3::new(0.09, 0.13, 0.07),
                4,
                0.1,
                0.5,
                |_| pal.hair,
            );
            held.torus(
                Vec3::new(0.0, 0.03, 0.07),
                Vec3::Y,
                0.065,
                0.014,
                10,
                pal.brass,
            );
            glows.blob(
                Vec3::new(0.0, 0.12, 0.07),
                Vec3::splat(0.09),
                4,
                0.0,
                0.0,
                |d| {
                    style.glow.lerp(pal.whitewash, d.y.max(0.0) * 0.4)
                },
            );
        }
        Stall::Community => {
            // A kerchief of its stall's cloth knotted behind, goggles
            // on its brow, a pencil behind its ear, and a hammer.
            head.blob(
                Vec3::new(0.0, 0.2, -0.03),
                Vec3::new(0.29, 0.16, 0.29),
                5,
                0.04,
                1.0,
                |_| style.cloth,
            );
            for side in [-1.0f32, 1.0] {
                head.ball(
                    Vec3::new(side * 0.05, 0.13, -0.3),
                    Vec3::new(0.05, 0.035, 0.04),
                    3,
                    style.cloth,
                );
                head.torus(
                    Vec3::new(side * 0.09, 0.17, 0.25),
                    Vec3::new(0.0, 0.5, 1.0),
                    0.05,
                    0.015,
                    10,
                    pal.leather,
                );
                head.ball(
                    Vec3::new(side * 0.09, 0.17, 0.25),
                    Vec3::new(0.04, 0.04, 0.012),
                    2,
                    pal.steel_bright,
                );
            }
            head.rod(
                Vec3::new(0.25, 0.02, 0.05),
                Vec3::new(0.27, 0.14, -0.14),
                0.012,
                5,
                pal.ochre,
                true,
            );
            held.rod(
                Vec3::Y * -0.08,
                Vec3::Y * 0.26,
                0.018,
                5,
                pal.timber,
                true,
            );
            held.rbox(
                Vec3::new(0.0, 0.27, 0.0),
                Vec3::new(0.08, 0.035, 0.035),
                0.01,
                Quat::IDENTITY,
                pal.steel,
                0.0,
            );
        }
        Stall::Plugins => {
            // Brass goggles pushed up on its brow on a leather strap,
            // white whiskers, and a great spanner.
            head.torus(
                Vec3::new(0.0, 0.16, 0.0),
                Vec3::new(0.0, 1.0, -0.25),
                0.275,
                0.02,
                16,
                pal.leather,
            );
            for side in [-1.0f32, 1.0] {
                let lens = Vec3::new(side * 0.1, 0.22, 0.2);
                let tilt = Vec3::new(0.0, 0.8, 0.6);
                head.torus(lens, tilt, 0.06, 0.02, 10, pal.brass);
                let glass = Affine3A::from_rotation_translation(
                    Quat::from_rotation_arc(
                        Vec3::Z,
                        tilt.normalize(),
                    ),
                    lens,
                );
                within(&mut head, glass, |mb| {
                    mb.rod(
                        Vec3::Z * -0.01,
                        Vec3::Z * 0.01,
                        0.048,
                        10,
                        pal.steel_bright,
                        true,
                    );
                });
                head.blob(
                    Vec3::new(side * 0.21, -0.08, 0.1),
                    Vec3::new(0.07, 0.11, 0.07),
                    4,
                    0.15,
                    side,
                    |_| pal.whitewash * 0.9,
                );
            }
            held.rbox(
                Vec3::new(0.0, 0.1, 0.0),
                Vec3::new(0.025, 0.2, 0.012),
                0.008,
                Quat::IDENTITY,
                pal.steel,
                0.0,
            );
            held.torus(
                Vec3::new(0.0, 0.33, 0.0),
                Vec3::Z,
                0.055,
                0.022,
                10,
                pal.steel,
            );
        }
        Stall::Skills => {
            // Round spectacles under its hood.
            for side in [-1.0f32, 1.0] {
                head.torus(
                    Vec3::new(side * 0.095, 0.0, 0.27),
                    Vec3::Z,
                    0.055,
                    0.009,
                    12,
                    pal.brass,
                );
                head.rod(
                    Vec3::new(side * 0.15, 0.01, 0.26),
                    Vec3::new(side * 0.25, 0.03, 0.08),
                    0.006,
                    3,
                    pal.brass,
                    false,
                );
            }
            head.rod(
                Vec3::new(-0.04, 0.01, 0.275),
                Vec3::new(0.04, 0.01, 0.275),
                0.006,
                3,
                pal.brass,
                false,
            );
        }
        Stall::Hamlets => {
            // A hat with a wide brim of its stall's cloth, a band of
            // its trim and a feather.
            let tip = Affine3A::from_rotation_translation(
                Quat::from_rotation_z(-0.12)
                    * Quat::from_rotation_x(-0.1),
                Vec3::Y * 0.2,
            );
            within(&mut head, tip, |head| {
                head.tube(
                    Vec3::ZERO,
                    Vec3::Y * 0.025,
                    0.46,
                    0.44,
                    20,
                    style.cloth,
                    true,
                );
                head.lathe(
                    Vec3::Y * 0.02,
                    &[
                        (0.0, 0.0),
                        (0.24, 0.0),
                        (0.23, 0.16),
                        (0.18, 0.2),
                        (0.0, 0.21),
                    ],
                    14,
                    |_, _| style.cloth,
                );
                head.torus(
                    Vec3::Y * 0.06,
                    Vec3::Y,
                    0.235,
                    0.03,
                    14,
                    style.trim,
                );
                head.blob(
                    Vec3::new(0.2, 0.2, -0.1),
                    Vec3::new(0.035, 0.22, 0.06),
                    4,
                    0.1,
                    2.0,
                    feather,
                );
            });
        }
    }
    Worn { head, held, glows }
}
