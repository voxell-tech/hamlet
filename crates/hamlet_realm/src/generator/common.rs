//! The common: a town's shared ground, as a paved round with a
//! notice board and a lantern. Built about the town's centre in its
//! own frame.

use std::f32::consts::TAU;

use bevy::prelude::*;

use super::house::TownStyle;
use super::land::Land;
use crate::lantern::lantern_cage;
use crate::meshes::{Meshing, Rng};

pub fn mesh(land: &Land, site: usize, style: &TownStyle) -> Meshing {
    let (pal, seed) = (&land.pal, land.seed);
    let town = &land.set.town;
    let (radius, lift, lamp) =
        (town.plaza_radius, town.plaza_lift, town.lamp);
    let mut rng = Rng::seeded(&format!("common {seed} {site}"));
    let mut meshing = Meshing::new();

    // Flagstones in rings, each a little off level, over a dark bed
    // that shows in the joints.
    let bed = pal.limestone_shade * 0.55;
    let rim = 28;
    let centre = meshing.v(Vec3::Y * (lift - 0.03), Vec3::Y, bed);
    let ring = (0..rim)
        .map(|i| {
            let a = i as f32 / rim as f32 * TAU;
            let r = radius * (1.0 + rng.sym(0.025));
            let p = Vec3::new(a.cos() * r, 0.0, a.sin() * r);
            (
                meshing.v(p + Vec3::Y * (lift - 0.03), Vec3::Y, bed),
                meshing.v(
                    p - Vec3::Y * 0.3,
                    p.normalize(),
                    bed * 0.8,
                ),
            )
        })
        .collect::<Vec<_>>();
    for i in 0..rim {
        let (a, b) = (ring[i], ring[(i + 1) % rim]);
        meshing.tri(centre, a.0, b.0);
        meshing.quad(a.0, b.0, b.1, a.1);
    }
    let rings = 7;
    for k in 0..rings {
        let (r0, r1) = (
            radius * k as f32 / rings as f32,
            radius * (k + 1) as f32 / rings as f32,
        );
        let count = if k == 0 { 1 } else { k * 6 };
        let phase = rng.f();
        for i in 0..count {
            let (a0, a1) = (
                (i as f32 + phase) / count as f32 * TAU,
                (i as f32 + 1.0 + phase) / count as f32 * TAU,
            );
            let col = pal
                .limestone
                .lerp(pal.limestone_shade, rng.f() * 0.4)
                * rng.value(0.05);
            let y = lift - rng.range(0.0, 0.018);
            let at = |a: f32, r: f32| {
                Vec3::new(a.cos() * r, y, a.sin() * r)
            };
            if k == 0 {
                let rim = (0..8)
                    .map(|j| {
                        let p = at(j as f32 / 8.0 * TAU, r1 - 0.04);
                        meshing.v(p, Vec3::Y, col)
                    })
                    .collect::<Vec<_>>();
                let m = meshing.v(at(0.0, 0.0), Vec3::Y, col);
                for j in 0..8 {
                    meshing.tri(m, rim[j], rim[(j + 1) % 8]);
                }
                continue;
            }
            let gap = 0.035 / r1;
            let mid = (a0 + a1) * 0.5;
            meshing.face(
                [
                    at(a0 + gap, r0 + 0.035),
                    at(mid, r0 + 0.035),
                    at(mid, r1 - 0.035),
                    at(a0 + gap, r1 - 0.035),
                ],
                Vec3::Y,
                col,
            );
            meshing.face(
                [
                    at(mid, r0 + 0.035),
                    at(a1 - gap, r0 + 0.035),
                    at(a1 - gap, r1 - 0.035),
                    at(mid, r1 - 0.035),
                ],
                Vec3::Y,
                col,
            );
        }
    }

    // A ring of groove at the rim, where the roads' grooves end, and
    // a spoke from the hall's door to a roundel in the middle.
    let y = lift + 0.014;
    let (slate, pewter) = (pal.bed, pal.steel);
    let arc = |meshing: &mut Meshing, r: f32, n: usize| {
        for i in 0..n {
            let (a0, a1) = (
                i as f32 / n as f32 * TAU,
                (i + 1) as f32 / n as f32 * TAU,
            );
            let at = |a: f32, r: f32, lift: f32| {
                Vec3::new(a.cos() * r, y + lift, a.sin() * r)
            };
            for (half, lift, col) in
                [(0.12, 0.0, slate), (0.04, 0.004, pewter)]
            {
                meshing.face(
                    [
                        at(a0, r - half, lift),
                        at(a1, r - half, lift),
                        at(a1, r + half, lift),
                        at(a0, r + half, lift),
                    ],
                    Vec3::Y,
                    col,
                );
            }
        }
    };
    arc(&mut meshing, radius - 0.3, 40);
    arc(&mut meshing, 0.8, 14);
    for (half, lift, col) in
        [(0.12, 0.0, slate), (0.04, 0.004, pewter)]
    {
        meshing.face(
            [
                Vec3::new(-half, y + lift, -3.0),
                Vec3::new(half, y + lift, -3.0),
                Vec3::new(half, y + lift, -0.8),
                Vec3::new(-half, y + lift, -0.8),
            ],
            Vec3::Y,
            col,
        );
    }

    // The board the town's notes are pinned to.
    let board = Vec3::new(town.board.0.x, lift, town.board.0.y);
    let turn = Quat::from_rotation_y(1.1);
    for side in [-1.0f32, 1.0] {
        meshing.rbox(
            board + turn * Vec3::new(side * 0.62, 0.75, 0.0),
            Vec3::new(0.05, 0.75, 0.05),
            0.02,
            turn * rng.wobble(1.5),
            pal.timber * rng.value(0.08),
            0.25,
        );
    }
    meshing.rbox(
        board + Vec3::Y * 1.05,
        Vec3::new(0.6, 0.4, 0.03),
        0.02,
        turn,
        pal.timber_dark,
        0.1,
    );
    for i in 0..5 {
        let at = Vec3::new(
            -0.38 + (i % 3) as f32 * 0.37 + rng.sym(0.04),
            0.92 + (i / 3) as f32 * 0.3 + rng.sym(0.03),
            0.035,
        );
        meshing.rbox(
            board + turn * at,
            Vec3::new(0.13, 0.11, 0.006),
            0.004,
            turn * Quat::from_rotation_z(rng.sym(0.12)),
            pal.parchment * rng.value(0.06),
            0.0,
        );
    }
    meshing.face(
        [
            board + turn * Vec3::new(-0.75, 1.5, -0.22),
            board + turn * Vec3::new(0.75, 1.5, -0.22),
            board + turn * Vec3::new(0.75, 1.62, 0.3),
            board + turn * Vec3::new(-0.75, 1.62, 0.3),
        ],
        Vec3::Y,
        style.roof,
    );

    // The lantern post.
    let foot = Vec3::new(lamp.x - 0.3, lift, lamp.z);
    meshing.rbox(
        foot + Vec3::Y * 1.0,
        Vec3::new(0.055, 1.0, 0.055),
        0.02,
        rng.wobble(1.0),
        pal.timber * rng.value(0.06),
        0.3,
    );
    meshing.tube(
        foot + Vec3::Y * 1.95,
        lamp + Vec3::Y * 0.25,
        0.016,
        0.016,
        5,
        pal.bed,
        true,
    );
    lantern_cage(&mut meshing, lamp, 0.085, 0.2, pal.brass);
    meshing.lathe(
        lamp + Vec3::Y * 0.1,
        &[(0.11, 0.0), (0.06, 0.07), (0.0, 0.1)],
        8,
        |_, _| pal.brass,
    );
    meshing
}

/// The common as one disc, for towns seen from far away.
pub fn far(meshing: &mut Meshing, land: &Land) {
    let pal = &land.pal;
    let (radius, lift) =
        (land.set.town.plaza_radius, land.set.town.plaza_lift);
    let col = pal.limestone.lerp(pal.limestone_shade, 0.2);
    let m = meshing.v(Vec3::Y * lift, Vec3::Y, col);
    let rim = (0..16)
        .map(|i| {
            let a = i as f32 / 16.0 * TAU;
            meshing.v(
                Vec3::new(a.cos(), 0.0, a.sin()) * radius
                    + Vec3::Y * lift,
                Vec3::Y,
                col,
            )
        })
        .collect::<Vec<_>>();
    for i in 0..16 {
        meshing.tri(m, rim[i], rim[(i + 1) % 16]);
    }
}
