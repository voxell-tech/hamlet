//! The harbor as meshes: a plank pier, a small sailing boat and the
//! dashes of the sea route. A pier and a boat have +Z as the way they
//! point out to sea, and rest on the water at `y = 0`.

use bevy::prelude::*;

use crate::meshes::{Meshing, smoothstep};
use crate::palette::Palette;

/// How far a pier reaches inland of the coast and out to sea, and
/// its width.
pub(super) const INLAND: f32 = 3.0;
pub(super) const SEAWARD: f32 = 16.0;
const WIDTH: f32 = 2.4;

/// How far past the head of a pier the sea route begins.
pub(super) const OFFING: f32 = 3.0;

/// How high the deck lies over the water.
pub(super) const DECK: f32 = 0.75;

/// The boat's length from stern to bow, and the way it lies off the
/// pier's side.
const LENGTH: f32 = 4.8;
pub(super) const ALONGSIDE: f32 = 2.5;

/// Where the boat lies on a pier, along it.
pub(super) const MOORED: f32 = SEAWARD - 3.6;

/// The planks and posts of a pier.
pub(super) fn pier(pal: &Palette) -> Meshing {
    let mut m = Meshing::new();
    let (inland, seaward) = (-INLAND, SEAWARD);
    let half = WIDTH * 0.5;
    // Planks across, with a hair of gap between them.
    let count = ((seaward - inland) / 0.56) as usize;
    for k in 0..count {
        let z = inland + (k as f32 + 0.5) * 0.56;
        let shade = 0.9 + 0.1 * ((k * 7) % 5) as f32 / 4.0;
        m.rbox(
            Vec3::new(0.0, DECK, z),
            Vec3::new(half, 0.06, 0.25),
            0.02,
            Quat::IDENTITY,
            pal.timber * shade,
            0.0,
        );
    }
    // Beams under the deck, along it.
    let mid = (inland + seaward) * 0.5;
    let long = (seaward - inland) * 0.5;
    for side in [-1.0, 1.0] {
        m.rbox(
            Vec3::new(side * (half - 0.2), DECK - 0.17, mid),
            Vec3::new(0.08, 0.1, long),
            0.02,
            Quat::IDENTITY,
            pal.timber_dark,
            0.0,
        );
    }
    // Posts down to the bed, in pairs, the last ones taller.
    let mut z = inland + 0.8;
    while z <= seaward - 0.2 {
        let end = z + 3.0 > seaward - 0.2;
        for side in [-1.0, 1.0] {
            let at = Vec3::new(side * (half + 0.1), 0.0, z);
            let top = DECK + if end { 0.95 } else { 0.4 };
            m.tube(
                at - Vec3::Y * 2.6,
                at + Vec3::Y * top,
                0.12,
                0.11,
                7,
                pal.bark,
                true,
            );
        }
        z += 3.0;
    }
    // A crate and a coil of rope at the head.
    m.rbox(
        Vec3::new(-0.55, DECK + 0.38, seaward - 1.5),
        Vec3::new(0.34, 0.32, 0.34),
        0.03,
        Quat::from_rotation_y(0.3),
        pal.timber_dark,
        0.0,
    );
    m.rbox(
        Vec3::new(0.5, DECK + 0.14, seaward - 2.4),
        Vec3::new(0.28, 0.1, 0.28),
        0.06,
        Quat::IDENTITY,
        pal.dirt_shade,
        0.0,
    );
    m
}

/// The half width of the hull at `t`, 0 at the stern and 1 at the
/// bow.
fn beam(t: f32) -> f32 {
    let taper = (1.0 - t.powf(2.5)).max(0.0).powf(0.6);
    0.85 * taper * (0.7 + 0.3 * smoothstep(0.0, 0.35, t))
}

/// A small boat under one sail, in the hull colour `paint`.
pub(super) fn boat(pal: &Palette, paint: Vec3) -> Meshing {
    let mut m = Meshing::new();
    const STATIONS: usize = 9;
    // From the gunwale on one side, down the keel, to the gunwale on
    // the other.
    let outer = |i: usize| -> [Vec3; 5] {
        let t = i as f32 / (STATIONS - 1) as f32;
        let z = (t - 0.4) * LENGTH;
        let (w, rise) = (beam(t), t * t);
        let keel = -0.3 + 0.35 * t.powi(3);
        let sheer = 0.38 + 0.34 * rise;
        [
            Vec3::new(-w, sheer, z),
            Vec3::new(-w * 0.55, keel + 0.12, z),
            Vec3::new(0.0, keel, z),
            Vec3::new(w * 0.55, keel + 0.12, z),
            Vec3::new(w, sheer, z),
        ]
    };
    // The inside is the same hull a plank's thickness in.
    let inner = |i: usize| -> [Vec3; 5] {
        let mut ring = outer(i);
        for p in &mut ring {
            p.x *= 0.88;
            if p.y < 0.3 {
                p.y += 0.06;
            }
        }
        ring
    };
    let axis = |p: Vec3| Vec3::new(0.0, 0.3, p.z);
    for i in 0..STATIONS - 1 {
        let (a, b) = (outer(i), outer(i + 1));
        let (c, d) = (inner(i), inner(i + 1));
        for j in 0..4 {
            let mid = (a[j] + b[j + 1]) * 0.5;
            m.face(
                [a[j], a[j + 1], b[j + 1], b[j]],
                mid - axis(mid),
                paint,
            );
            let mid = (c[j] + d[j + 1]) * 0.5;
            m.face(
                [c[j], c[j + 1], d[j + 1], d[j]],
                axis(mid) - mid,
                pal.timber,
            );
        }
        // The rim, from the outer gunwale to the inner one.
        for j in [0, 4] {
            m.face(
                [a[j], c[j], d[j], b[j]],
                Vec3::Y,
                pal.timber_dark,
            );
        }
    }
    // The transom.
    let (a, c) = (outer(0), inner(0));
    m.face([a[0], a[1], a[3], a[4]], -Vec3::Z, paint);
    m.face([a[1], a[2], a[3], a[3]], -Vec3::Z, paint);
    m.face([c[0], c[1], c[3], c[4]], Vec3::Z, pal.timber);
    // Seats across.
    for z in [-1.2, 0.0] {
        m.rbox(
            Vec3::new(0.0, 0.18, z),
            Vec3::new(beam(0.45) * 0.85, 0.04, 0.16),
            0.01,
            Quat::IDENTITY,
            pal.timber_dark,
            0.0,
        );
    }
    // A mast and a sail on it, seen from both sides.
    let (foot, top, boom) = (
        Vec3::new(0.0, 0.2, 0.7),
        Vec3::new(0.0, 3.6, 0.7),
        Vec3::new(0.0, 0.9, -1.5),
    );
    m.tube(foot, top, 0.05, 0.035, 6, pal.bark, true);
    m.tube(
        Vec3::new(0.0, 0.9, 0.7),
        boom,
        0.03,
        0.03,
        5,
        pal.bark,
        true,
    );
    let sail = [
        Vec3::new(0.0, 0.95, 0.7),
        top - Vec3::Y * 0.2,
        boom + Vec3::Y * 0.05,
        boom + Vec3::Y * 0.05,
    ];
    for side in [-1.0, 1.0] {
        let mut belly = sail;
        belly[1].x = side * 0.04;
        m.face(belly, Vec3::X * side, pal.whitewash);
    }
    m
}

/// A line of dashes along +Z from the origin, `length` long, `width`
/// across, lying flat.
pub(super) fn route(length: f32, width: f32) -> Meshing {
    let mut m = Meshing::new();
    let (dash, gap) = (6.0, 10.0);
    let mut z = 0.0;
    while z < length {
        let end = (z + dash).min(length);
        let half = width * 0.5;
        m.face(
            [
                Vec3::new(-half, 0.0, z),
                Vec3::new(half, 0.0, z),
                Vec3::new(half, 0.0, end),
                Vec3::new(-half, 0.0, end),
            ],
            Vec3::Y,
            Vec3::ONE,
        );
        z += dash + gap;
    }
    m
}
