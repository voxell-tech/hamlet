//! The parts every robot has, and the mayor's dress.

use bevy::prelude::*;

use super::Piece;
use crate::keeper::Emblem;
use crate::keeper::pose::{FACE, HOOK, NECK, POLE_FOOT, WHEEL};
use crate::lantern;
use crate::meshes::Meshing;
use crate::palette::Palette;

/// Near white, so the paint material gives the colour.
pub(super) fn torso(pal: &Palette) -> Piece {
    let mut p = Piece::default();
    let staves = [
        (0.0, 0.0),
        (0.165, 0.0),
        (0.215, 0.05),
        (0.245, 0.19),
        (0.238, 0.31),
        (0.19, 0.415),
        (0.1, 0.44),
        (0.0, 0.44),
    ];
    p.wood.lathe(Vec3::ZERO, &staves, 14, |_, s| {
        Vec3::splat(1.0 + 0.1 * ((s * 5 % 7) as f32 / 7.0 - 0.5))
    });
    for (y, r) in [(0.075, 0.228), (0.345, 0.232)] {
        p.brass.torus(Vec3::Y * y, Vec3::Y, r, 0.02, 20, pal.brass);
    }
    // The plate its emblem sits on.
    p.brass.rbox(
        Vec3::new(0.0, 0.21, 0.238),
        Vec3::new(0.05, 0.065, 0.012),
        0.012,
        Quat::IDENTITY,
        pal.brass,
        0.0,
    );
    // The collar the head sits on.
    let collar = [
        (0.11, 0.0),
        (0.12, 0.015),
        (0.075, 0.035),
        (0.07, 0.06),
        (0.0, 0.06),
    ];
    p.brass.lathe(Vec3::Y * (NECK - 0.02), &collar, 12, |_, _| {
        pal.brass * 0.9
    });
    p
}

/// The winding key in its back, about its own axis.
pub(super) fn key(pal: &Palette) -> Meshing {
    let mut meshing = Meshing::new();
    meshing.tube(
        Vec3::Z * -0.2,
        Vec3::Z * -0.36,
        0.018,
        0.018,
        8,
        pal.brass,
        false,
    );
    meshing.torus(
        Vec3::Z * -0.42,
        Vec3::X,
        0.065,
        0.016,
        14,
        pal.brass,
    );
    meshing
}

pub(in crate::keeper) const KEY_AT: Vec3 = Vec3::new(0.0, 0.25, 0.0);

pub(super) fn emblem(which: Emblem, pal: &Palette) -> Meshing {
    let mut meshing = Meshing::new();
    let at = Vec3::new(0.0, 0.21, 0.25);
    let mut bar = |c: Vec3, half: Vec2, turn: f32| {
        meshing.rbox(
            at + c,
            Vec3::new(half.x, half.y, 0.006),
            0.003,
            Quat::from_rotation_z(turn),
            pal.ink,
            0.0,
        );
    };
    match which {
        Emblem::Keyhole => {
            bar(Vec3::Y * -0.015, Vec2::new(0.007, 0.022), 0.0);
            meshing.blob(
                at + Vec3::Y * 0.015,
                Vec3::new(0.016, 0.016, 0.008),
                4,
                0.0,
                0.0,
                |_| pal.ink,
            );
        }
        Emblem::Dot => meshing.blob(
            at,
            Vec3::new(0.024, 0.024, 0.008),
            5,
            0.0,
            0.0,
            |_| pal.ink,
        ),
        Emblem::Bars => {
            bar(Vec3::Y * 0.02, Vec2::new(0.03, 0.008), 0.0);
            bar(Vec3::Y * -0.02, Vec2::new(0.03, 0.008), 0.0);
        }
        Emblem::Diamond => {
            bar(Vec3::ZERO, Vec2::splat(0.022), 0.785);
        }
        Emblem::Plus => {
            bar(Vec3::ZERO, Vec2::new(0.03, 0.008), 0.0);
            bar(Vec3::ZERO, Vec2::new(0.008, 0.03), 0.0);
        }
        Emblem::Chevron => {
            bar(Vec3::X * -0.014, Vec2::new(0.024, 0.008), 0.7);
            bar(Vec3::X * 0.014, Vec2::new(0.024, 0.008), -0.7);
        }
    }
    meshing
}

pub(super) fn arm(pal: &Palette) -> Piece {
    let mut p = Piece::default();
    p.brass
        .blob(Vec3::ZERO, Vec3::splat(0.058), 6, 0.0, 0.0, |_| {
            pal.brass
        });
    p.wood.tube(
        Vec3::Y * -0.03,
        Vec3::Y * -0.27,
        0.04,
        0.034,
        8,
        pal.timber,
        false,
    );
    p.brass.blob(
        Vec3::Y * -0.3,
        Vec3::new(0.052, 0.06, 0.052),
        6,
        0.0,
        0.0,
        |_| pal.brass,
    );
    p
}

pub(super) fn leg(pal: &Palette) -> Piece {
    let mut p = Piece::default();
    p.wood.tube(
        Vec3::Y * 0.04,
        Vec3::Y * -0.22,
        0.058,
        0.05,
        8,
        pal.timber,
        false,
    );
    p.brass.blob(
        Vec3::new(0.0, -0.25, 0.035),
        Vec3::new(0.085, 0.055, 0.125),
        6,
        0.0,
        0.0,
        |_| pal.brass,
    );
    p
}

/// The two wheels of one foot, about their axle. A bar across each
/// shows it turning.
pub(super) fn wheels(pal: &Palette) -> Meshing {
    let mut meshing = Meshing::new();
    for side in [-1.0f32, 1.0] {
        let at = |x: f32| Vec3::X * (side * x);
        meshing.tube(
            at(0.082),
            at(0.106),
            WHEEL,
            WHEEL,
            12,
            pal.pewter * 0.55,
            true,
        );
        meshing.tube(
            at(0.1),
            at(0.114),
            0.02,
            0.016,
            6,
            pal.brass,
            true,
        );
        let bar = Vec3::Y * (WHEEL - 0.008);
        meshing.tube(
            at(0.107) - bar,
            at(0.107) + bar,
            0.007,
            0.007,
            4,
            pal.brass,
            false,
        );
    }
    meshing
}

pub(super) fn head(pal: &Palette) -> Meshing {
    let mut meshing = Meshing::new();
    lantern::lantern_cage(
        &mut meshing,
        Vec3::Y * FACE,
        0.145,
        0.22,
        pal.brass,
    );
    meshing
}

/// One dark eye on the glass, set in front of the head's axis.
pub(super) fn eye(pal: &Palette) -> Meshing {
    let mut meshing = Meshing::new();
    meshing.blob(
        Vec3::new(0.0, 0.015, 0.142),
        Vec3::new(0.021, 0.034, 0.012),
        5,
        0.0,
        0.0,
        |_| pal.ink,
    );
    meshing
}

pub(super) fn pole(pal: &Palette) -> Piece {
    let mut p = Piece::default();
    let wood = pal.timber_dark;
    p.wood.tube(
        Vec3::Y * -POLE_FOOT,
        Vec3::Y * 1.08,
        0.021,
        0.017,
        7,
        wood,
        true,
    );
    // The crook, bent forward to carry the lamp clear of the head.
    let bend = [
        Vec3::new(0.0, 1.08, 0.0),
        Vec3::new(0.0, 1.16, 0.05),
        Vec3::new(0.0, 1.18, 0.14),
        HOOK,
    ];
    for w in bend.windows(2) {
        p.wood.tube(w[0], w[1], 0.017, 0.016, 7, wood, false);
    }
    p.brass.torus(
        Vec3::Y * -0.4,
        Vec3::Y,
        0.022,
        0.008,
        8,
        pal.brass,
    );
    p
}

/// The carried lantern's centre below the hook.
pub(in crate::keeper) const LAMP_DROP: Vec3 =
    Vec3::new(0.0, -0.26, 0.0);

/// The carried lantern's frame and chain, hanging from the origin.
pub(super) fn lamp(pal: &Palette) -> Meshing {
    let mut meshing = Meshing::new();
    meshing.tube(
        Vec3::ZERO,
        LAMP_DROP + Vec3::Y * 0.2,
        0.006,
        0.006,
        5,
        pal.brass,
        false,
    );
    lantern::lantern_frame(
        &mut meshing,
        LAMP_DROP,
        0.08,
        0.15,
        pal.brass,
    );
    meshing
}

/// The mayor's short cape, over the shoulders of the body.
pub(super) fn mantle(pal: &Palette) -> Meshing {
    let mut meshing = Meshing::new();
    let cloth = pal.mantle;
    let cape = [
        (0.258, 0.322),
        (0.276, 0.328),
        (0.262, 0.355),
        (0.212, 0.428),
        (0.125, 0.452),
    ];
    meshing.lathe(Vec3::ZERO, &cape, 18, |r, s| {
        // A scalloped hem.
        cloth * if r == 0 && s % 2 == 0 { 0.8 } else { 1.0 }
    });
    meshing.blob(
        Vec3::new(0.0, 0.41, 0.225),
        Vec3::new(0.03, 0.03, 0.014),
        4,
        0.0,
        0.0,
        |_| pal.brass * 0.8,
    );
    meshing
}

/// The mayor's pennant, streaming back from the origin. Both faces
/// are drawn.
pub(super) fn pennant(pal: &Palette) -> Meshing {
    let mut meshing = Meshing::new();
    let cloth = pal.mantle;
    let pts =
        [Vec3::ZERO, Vec3::Y * -0.13, Vec3::new(0.0, -0.065, -0.27)];
    for n in [Vec3::X, Vec3::NEG_X] {
        let v = pts.map(|p| meshing.v(p + n * 0.001, n, cloth));
        meshing.tri(v[0], v[1], v[2]);
        let stripe = [
            Vec3::new(0.0, -0.035, -0.06),
            Vec3::new(0.0, -0.095, -0.06),
            Vec3::new(0.0, -0.065, -0.17),
        ]
        .map(|p| meshing.v(p + n * 0.002, n, pal.ink));
        meshing.tri(stripe[0], stripe[1], stripe[2]);
    }
    meshing
}
