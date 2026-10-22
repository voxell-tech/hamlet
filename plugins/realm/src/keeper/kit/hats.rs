//! The hats.

use glam::{Quat, Vec3};

use crate::keeper::kit::Piece;
use crate::keeper::who::Hat;
use crate::mesh::Mb;
use crate::palette::Palette;

/// The top of the leader's spire, above the hat's base.
pub(in crate::keeper) const SPIRE: f32 = 0.44;

/// A flat ring of brim from radius `a` out to `b`.
fn brim(mb: &mut Mb, a: f32, b: f32, y: f32, col: Vec3) {
    let ring =
        [(a, y), (b, y), (b + 0.004, y + 0.012), (a, y + 0.022)];
    mb.lathe(Vec3::ZERO, &ring, 16, |_, _| col);
}

/// A hat with its base at the origin. The wood half takes the
/// robot's paint.
pub(super) fn hat(which: Hat, pal: &Palette) -> Piece {
    let mut p = Piece::default();
    let felt = Vec3::splat(0.92);
    let band = |mb: &mut Mb, y: f32, r: f32| {
        mb.torus(Vec3::Y * y, Vec3::Y, r, 0.012, 18, pal.brass);
    };
    match which {
        Hat::Dome => {
            let dome = [
                (0.185, 0.0),
                (0.191, 0.014),
                (0.185, 0.032),
                (0.167, 0.093),
                (0.12, 0.143),
                (0.06, 0.171),
                (0.0, 0.178),
            ];
            p.brass.lathe(Vec3::ZERO, &dome, 16, |_, _| pal.brass);
            let flue = [
                (0.032, 0.0),
                (0.032, 0.032),
                (0.051, 0.037),
                (0.042, 0.057),
                (0.0, 0.061),
            ];
            p.brass.lathe(Vec3::Y * 0.171, &flue, 10, |_, _| {
                pal.brass * 0.9
            });
            // A visor, as a brim over the face.
            p.brass.rbox(
                Vec3::new(0.0, 0.008, 0.19),
                Vec3::new(0.115, 0.013, 0.055),
                0.01,
                Quat::from_rotation_x(0.28),
                pal.brass * 0.9,
                0.0,
            );
            p.brass.tube(
                Vec3::Y * 0.22,
                Vec3::Y * SPIRE,
                0.008,
                0.006,
                6,
                pal.brass,
                true,
            );
            p.brass.blob(
                Vec3::Y * SPIRE,
                Vec3::splat(0.018),
                4,
                0.0,
                0.0,
                |_| pal.brass,
            );
        }
        Hat::Cap => {
            let crown = [
                (0.17, 0.0),
                (0.186, 0.022),
                (0.18, 0.055),
                (0.13, 0.09),
                (0.0, 0.1),
            ];
            p.wood.lathe(Vec3::ZERO, &crown, 16, |_, _| felt);
            p.wood.rbox(
                Vec3::new(0.0, 0.01, 0.2),
                Vec3::new(0.12, 0.012, 0.075),
                0.01,
                Quat::from_rotation_x(0.22),
                felt * 0.8,
                0.0,
            );
            p.brass.blob(
                Vec3::Y * 0.1,
                Vec3::new(0.026, 0.014, 0.026),
                4,
                0.0,
                0.0,
                |_| pal.brass,
            );
        }
        Hat::Cone => {
            let roof = [
                (0.0, 0.0),
                (0.275, 0.0),
                (0.28, 0.012),
                (0.15, 0.13),
                (0.035, 0.245),
                (0.0, 0.255),
            ];
            p.wood.lathe(Vec3::ZERO, &roof, 16, |r, _| {
                felt * if r == 3 { 0.86 } else { 1.0 }
            });
            p.brass.blob(
                Vec3::Y * 0.262,
                Vec3::splat(0.026),
                4,
                0.0,
                0.0,
                |_| pal.brass,
            );
        }
        Hat::Bowler => {
            let crown = [
                (0.16, 0.0),
                (0.166, 0.06),
                (0.15, 0.125),
                (0.1, 0.168),
                (0.0, 0.182),
            ];
            p.wood.lathe(Vec3::ZERO, &crown, 16, |_, _| felt);
            brim(&mut p.wood, 0.15, 0.24, 0.0, felt * 0.85);
            band(&mut p.brass, 0.035, 0.166);
        }
        Hat::Pipe => {
            let pipe = [
                (0.125, 0.0),
                (0.125, 0.22),
                (0.148, 0.226),
                (0.148, 0.255),
                (0.0, 0.255),
            ];
            p.wood.lathe(Vec3::ZERO, &pipe, 14, |_, _| felt);
            brim(&mut p.wood, 0.12, 0.205, 0.0, felt * 0.85);
            band(&mut p.brass, 0.045, 0.13);
        }
        Hat::Knit => {
            let wool = [
                (0.172, 0.0),
                (0.184, 0.02),
                (0.184, 0.05),
                (0.172, 0.06),
                (0.165, 0.1),
                (0.115, 0.158),
                (0.0, 0.178),
            ];
            p.wood.lathe(Vec3::ZERO, &wool, 16, |r, s| {
                felt * if r < 3 {
                    0.78
                } else if s % 2 == 0 {
                    1.0
                } else {
                    0.9
                }
            });
            p.wood.blob(
                Vec3::Y * 0.2,
                Vec3::splat(0.05),
                5,
                0.12,
                3.0,
                |_| Vec3::splat(1.25),
            );
        }
        Hat::Straw => {
            let crown = [
                (0.15, 0.0),
                (0.152, 0.085),
                (0.13, 0.108),
                (0.0, 0.114),
            ];
            p.wood.lathe(Vec3::ZERO, &crown, 16, |_, _| felt);
            brim(&mut p.wood, 0.14, 0.31, 0.0, felt * 0.9);
            band(&mut p.brass, 0.04, 0.155);
        }
    }
    p
}
