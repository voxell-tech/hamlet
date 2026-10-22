//! The things a robot holds in its free hand.

use glam::{Quat, Vec3};

use crate::build::mesh::Mb;
use crate::folk::keeper::pose::Prop;
use crate::world::palette::Palette;

/// The thing held in the free hand, in that arm's space. The hand is
/// 0.3 below the shoulder.
pub(super) fn prop(which: Prop, pal: &Palette) -> Mb {
    let mut mb = Mb::new();
    let wax = pal.wax;
    match which {
        Prop::None => {}
        Prop::Warrant => {
            mb.rbox(
                Vec3::new(0.0, -0.42, 0.02),
                Vec3::new(0.085, 0.11, 0.006),
                0.004,
                Quat::IDENTITY,
                pal.parchment,
                0.0,
            );
            mb.blob(
                Vec3::new(0.035, -0.47, 0.03),
                Vec3::new(0.022, 0.022, 0.008),
                4,
                0.0,
                0.0,
                |_| wax,
            );
            for (i, w) in
                [0.05f32, 0.055, 0.035].into_iter().enumerate()
            {
                mb.rbox(
                    Vec3::new(-0.01, -0.36 - 0.03 * i as f32, 0.027),
                    Vec3::new(w, 0.005, 0.002),
                    0.001,
                    Quat::IDENTITY,
                    pal.ink,
                    0.0,
                );
            }
        }
        Prop::Letter => {
            let c = Vec3::new(0.0, -0.39, 0.03);
            mb.rbox(
                c,
                Vec3::new(0.1, 0.068, 0.007),
                0.004,
                Quat::IDENTITY,
                pal.parchment,
                0.0,
            );
            // The flap, and the seal that holds it.
            for side in [-1.0f32, 1.0] {
                mb.rbox(
                    c + Vec3::new(side * 0.047, 0.03, 0.008),
                    Vec3::new(0.058, 0.004, 0.002),
                    0.001,
                    Quat::from_rotation_z(side * 0.62),
                    pal.parchment * 0.72,
                    0.0,
                );
            }
            mb.blob(
                c + Vec3::new(0.0, -0.005, 0.01),
                Vec3::new(0.02, 0.02, 0.008),
                4,
                0.0,
                0.0,
                |_| wax,
            );
        }
        Prop::Mallet => {
            mb.tube(
                Vec3::Y * -0.26,
                Vec3::new(0.0, -0.5, 0.02),
                0.016,
                0.016,
                6,
                pal.timber,
                true,
            );
            mb.tube(
                Vec3::new(0.0, -0.52, -0.075),
                Vec3::new(0.0, -0.52, 0.115),
                0.055,
                0.055,
                10,
                pal.timber_dark,
                true,
            );
        }
        Prop::Block => {
            mb.rbox(
                Vec3::new(0.17, -0.36, 0.0),
                Vec3::new(0.13, 0.085, 0.09),
                0.02,
                Quat::IDENTITY,
                pal.limestone,
                0.25,
            );
        }
        Prop::Ballot => {
            let c = Vec3::new(0.0, -0.39, 0.02);
            mb.rbox(
                c,
                Vec3::new(0.06, 0.075, 0.006),
                0.004,
                Quat::IDENTITY,
                pal.parchment,
                0.0,
            );
            // A tick.
            for (x, y, len, turn) in [
                (-0.018, -0.012, 0.016, -0.8f32),
                (0.012, 0.004, 0.034, 0.9),
            ] {
                mb.rbox(
                    c + Vec3::new(x, y, 0.008),
                    Vec3::new(len, 0.006, 0.002),
                    0.001,
                    Quat::from_rotation_z(turn),
                    pal.ink,
                    0.0,
                );
            }
        }
        Prop::Book => {
            let c = Vec3::new(0.0, -0.37, 0.02);
            mb.rbox(
                c - Vec3::Z * 0.006,
                Vec3::new(0.165, 0.115, 0.008),
                0.006,
                Quat::IDENTITY,
                pal.leather,
                0.0,
            );
            // Two pages, open in a shallow V.
            for side in [-1.0f32, 1.0] {
                let turn = Quat::from_rotation_y(-side * 0.2);
                let page = c + Vec3::new(side * 0.078, 0.0, 0.016);
                mb.rbox(
                    page,
                    Vec3::new(0.074, 0.1, 0.007),
                    0.004,
                    turn,
                    pal.parchment,
                    0.0,
                );
                for i in 0..3 {
                    mb.rbox(
                        page + turn
                            * Vec3::new(
                                0.0,
                                0.05 - 0.045 * i as f32,
                                0.008,
                            ),
                        Vec3::new(0.05, 0.005, 0.002),
                        0.001,
                        turn,
                        pal.ink,
                        0.0,
                    );
                }
            }
        }
        Prop::Quill => {
            mb.tube(
                QUILL_NIB,
                QUILL_NIB + QUILL * 0.52,
                0.007,
                0.004,
                5,
                pal.ink,
                true,
            );
            // The vane, flat and white.
            mb.rbox(
                QUILL_NIB + QUILL * 0.32,
                Vec3::new(0.062, 0.006, 0.19),
                0.004,
                Quat::from_rotation_x(-QUILL.y.atan2(QUILL.z)),
                pal.page,
                0.0,
            );
        }
        Prop::Scroll => {
            let turn =
                Quat::from_rotation_z(SCROLL.y.atan2(SCROLL.x));
            let across = turn * Vec3::Y;
            let top = Vec3::new(0.0, -0.31, 0.0);
            mb.rbox(
                top + SCROLL * 0.29,
                Vec3::new(0.29, 0.1, 0.004),
                0.003,
                turn,
                pal.parchment,
                0.0,
            );
            for k in [0.0f32, 0.58] {
                let c = top + SCROLL * k;
                mb.tube(
                    c - across * 0.13,
                    c + across * 0.13,
                    0.024,
                    0.024,
                    7,
                    pal.timber_dark,
                    true,
                );
            }
            for i in 0..6 {
                let w = if i % 3 == 2 { 0.045 } else { 0.07 };
                mb.rbox(
                    top + SCROLL * (0.09 + 0.075 * i as f32)
                        + Vec3::Z * 0.006,
                    Vec3::new(0.005, w, 0.002),
                    0.001,
                    turn,
                    pal.ink,
                    0.0,
                );
            }
        }
        Prop::Kite => {
            let far = Vec3::Y * -KITE;
            let cloth = [pal.parchment, pal.kite];
            let tips = [
                far + Vec3::Y * -0.6,
                far + Vec3::new(0.44, -0.1, 0.0),
                far + Vec3::Y * 0.7,
                far + Vec3::new(-0.44, -0.1, 0.0),
            ];
            let mid = far + Vec3::Y * -0.1;
            for n in [Vec3::Z, Vec3::NEG_Z] {
                for i in 0..4 {
                    let col = cloth[i % 2];
                    let v = [mid, tips[i], tips[(i + 1) % 4]]
                        .map(|p| mb.v(p + n * 0.004, n, col));
                    mb.tri(v[0], v[1], v[2]);
                }
            }
            mb.tube(
                tips[0],
                tips[2],
                0.008,
                0.008,
                4,
                pal.timber_dark,
                false,
            );
            mb.tube(
                tips[1],
                tips[3],
                0.008,
                0.008,
                4,
                pal.timber_dark,
                false,
            );
            // A tail of bows, and the string down to the hand.
            let mut at = tips[2];
            for i in 0..4 {
                let side = if i % 2 == 0 { 1.0 } else { -1.0 };
                let next = at + Vec3::new(side * 0.09, 0.17, 0.0);
                mb.tube(at, next, 0.005, 0.005, 3, pal.ink, false);
                mb.rbox(
                    next,
                    Vec3::new(0.06, 0.028, 0.004),
                    0.003,
                    Quat::from_rotation_z(side * 0.6),
                    cloth[(i + 1) % 2],
                    0.0,
                );
                at = next;
            }
            mb.tube(
                Vec3::Y * -0.3,
                mid,
                0.006,
                0.006,
                3,
                pal.ink,
                false,
            );
        }
    }
    mb
}

/// The nib of the quill, and the way its feather points, in the
/// arm's space.
const QUILL_NIB: Vec3 = Vec3::new(0.0, -0.37, 0.0);
const QUILL: Vec3 = Vec3::new(0.0, 0.507, 0.862);
/// The way a scroll hangs from a hand held high and out to the side.
const SCROLL: Vec3 = Vec3::new(0.81, 0.59, 0.0);
/// Height a kite flies up its string.
pub(in crate::folk::keeper) const KITE: f32 = 4.2;

/// The part of a prop that shows the robot's light, in the arm's
/// space. Empty for a prop that has none.
pub(super) fn glint(which: Prop) -> Mb {
    let mut mb = Mb::new();
    let mut spark = |at: Vec3, r: f32| {
        mb.blob(at, Vec3::splat(r), 4, 0.0, 0.0, |_| Vec3::ONE);
    };
    match which {
        Prop::Quill => spark(QUILL_NIB, 0.024),
        Prop::Scroll => {
            spark(Vec3::new(0.0, -0.31, 0.012) + SCROLL * 0.5, 0.03)
        }
        // A bead of light on the string.
        Prop::Kite => spark(Vec3::Y * -0.3, 0.07),
        _ => {}
    }
    mb
}
