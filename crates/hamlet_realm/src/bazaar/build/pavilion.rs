//! The bazaar's middle as meshes: its paving, the pavilion with its
//! counter, and the gate at its front.

use std::f32::consts::{FRAC_PI_4, TAU};

use bevy::prelude::*;

use super::{
    FLOOR, GATE, PAVED, PAVILION, POLE, POSTS, RIM, SHADE, STRIPES,
    Shapes, lantern, ring, stripe,
};
use crate::meshes::Meshing;
use crate::palette::Palette;

/// The paving: a border of tiles, tiles round the pavilion's foot,
/// a star in the plaza before it, stepping stones from the gate, and
/// a rug in the pavilion.
pub(super) fn ground(mb: &mut Meshing, pal: &Palette) {
    mb.rod(
        Vec3::ZERO,
        Vec3::Y * 0.1,
        PAVED + 0.45,
        48,
        pal.limestone_shade,
        true,
    );
    mb.rod(Vec3::ZERO, Vec3::Y * FLOOR, PAVED, 48, pal.dirt, true);
    let tiles =
        [pal.terracotta, pal.limestone, pal.ochre, pal.limestone];
    let rim = Vec3::Y * (FLOOR + 0.004);
    ring(mb, rim, (PAVED - 0.5, PAVED - 0.08), 72, |s| {
        tiles[s % tiles.len()]
    });
    ring(mb, rim, (PAVED - 0.82, PAVED - 0.7), 72, |_| {
        pal.terracotta
    });
    // A border of tiles round the pavilion's foot.
    ring(mb, rim, (RIM.0 + 0.1, RIM.0 + 0.4), 48, |s| {
        tiles[(s + 1) % tiles.len()]
    });
    // The star in the plaza before the pavilion, of two squares
    // turned on each other.
    let star = Vec3::new(0.0, FLOOR, 6.6);
    ring(mb, star + Vec3::Y * 0.004, (0.95, 1.12), 24, |s| {
        [pal.terracotta, pal.ochre][s % 2]
    });
    ring(mb, star + Vec3::Y * 0.004, (0.0, 0.95), 24, |_| {
        pal.limestone
    });
    for (turn, colour, lift) in [
        (0.0, pal.awnings[2], 0.008),
        (FRAC_PI_4, pal.awnings[3], 0.012),
    ] {
        mb.plate(
            star + Vec3::Y * lift,
            Vec3::new(0.6, 0.004, 0.6),
            Quat::from_rotation_y(turn),
            colour,
        );
    }
    ring(mb, star + Vec3::Y * 0.018, (0.0, 0.24), 12, |_| pal.brass);
    for z in [10.55f32, 9.9, 8.3] {
        mb.rbox(
            Vec3::new(0.0, FLOOR + 0.01, z),
            Vec3::new(0.42, 0.012, 0.22),
            0.01,
            Quat::from_rotation_y(z * 0.7),
            pal.limestone,
            0.0,
        );
    }
    let rug = PAVILION + Vec3::Z * 0.4;
    mb.slab(
        rug + Vec3::Y * (FLOOR + 0.006),
        Vec3::new(2.7, 0.008, 2.1),
        pal.awnings[1],
    );
    mb.slab(
        rug + Vec3::Y * (FLOOR + 0.012),
        Vec3::new(2.5, 0.01, 1.9),
        pal.awnings[2],
    );
}

/// The spot of the `k`th of the pavilion's eight posts, the first
/// at the front a little to the right.
fn post(k: usize) -> Vec3 {
    let a = (k as f32 + 0.5) / 8.0 * TAU;
    PAVILION + Vec3::new(a.sin(), 0.0, a.cos()) * POSTS
}

/// The spot on the pavilion's rim at the edge of the `s`th stripe,
/// `below` under the rim.
pub(super) fn rim_at(s: usize, out: f32, below: f32) -> Vec3 {
    let t = s as f32 / STRIPES as f32 * TAU;
    let (rim, eave) = RIM;
    PAVILION
        + Vec3::new(
            t.cos() * (rim + out),
            eave - below,
            t.sin() * (rim + out),
        )
}

/// The pavilion: its poles, its striped canopy and cupola, the
/// curtains round its back, its lanterns and its counter. Returns
/// the spots of its lanterns.
pub(super) fn pavilion(
    mb: &mut Meshing,
    glass: &mut Meshing,
    pal: &Palette,
) -> Vec<Vec3> {
    let (rim, eave) = RIM;
    mb.tube(
        PAVILION + Vec3::Y * FLOOR,
        PAVILION + Vec3::Y * POLE,
        0.12,
        0.06,
        8,
        pal.timber_dark,
        true,
    );
    for k in 0..8 {
        mb.tube(
            post(k) + Vec3::Y * FLOOR,
            post(k) + Vec3::Y * (eave + 0.1),
            0.09,
            0.08,
            6,
            pal.timber,
            true,
        );
        mb.ball(
            post(k) + Vec3::Y * (eave + 0.12),
            Vec3::splat(0.1),
            3,
            pal.brass,
        );
    }
    // The canopy dips between its rim and its peak, as a cloth
    // pulled up a pole does. Inside, it is the same cloth in shade.
    let canopy = [
        (rim, 0.0),
        (rim * 0.87, 0.55),
        (rim * 0.68, 1.3),
        (rim * 0.45, 2.2),
        (rim * 0.22, 3.0),
        (0.0, 3.55),
    ];
    let base = PAVILION + Vec3::Y * eave;
    mb.lathe(base, &canopy, STRIPES, |_, s| stripe(pal, 0, s));
    let mut under = canopy.map(|(r, y)| (r * 0.97, y - 0.05));
    under.reverse();
    mb.lathe(base, &under, STRIPES, |_, s| stripe(pal, 0, s) * SHADE);
    // A brass band round the rim, and a scalloped edge under it, a
    // flap to each stripe.
    mb.torus(base, Vec3::Y, rim + 0.01, 0.035, 32, pal.brass);
    for s in 0..STRIPES {
        let (left, right) =
            (rim_at(s, 0.0, 0.0), rim_at(s + 1, 0.0, 0.0));
        let out =
            ((left + right) * 0.5 - PAVILION).with_y(0.0).normalize();
        let tip =
            PAVILION + out * (rim + 0.02) + Vec3::Y * (eave - 0.42);
        let colour = [pal.awnings[1], pal.whitewash][s % 2];
        mb.flap((left, right), tip, out, colour);
        mb.ball(tip, Vec3::splat(0.035), 2, pal.brass);
    }
    // A striped cupola at the peak, where the canopy meets the pole.
    let cupola = [
        (0.5, 0.0),
        (0.5, 0.22),
        (0.42, 0.4),
        (0.24, 0.58),
        (0.0, 0.72),
    ];
    mb.lathe(
        PAVILION + Vec3::Y * (eave + 3.33),
        &cupola,
        8,
        |_, s| stripe(pal, 2, s),
    );
    mb.ball(
        PAVILION + Vec3::Y * (eave + 4.1),
        Vec3::splat(0.13),
        4,
        pal.brass,
    );
    // Curtains in stripes round the sides and the back. The front
    // is open.
    for k in 1..6 {
        let (a, b) = (post(k), post(k + 1));
        let out = ((a + b) * 0.5 - PAVILION).with_y(0.0);
        for strip in 0..3 {
            let (s, t) =
                (strip as f32 / 3.0, (strip + 1) as f32 / 3.0);
            let (p, q) = (a.lerp(b, s), a.lerp(b, t));
            mb.cloth(
                [
                    p + Vec3::Y * FLOOR,
                    q + Vec3::Y * FLOOR,
                    q + Vec3::Y * eave,
                    p + Vec3::Y * eave,
                ],
                out,
                stripe(pal, 2, strip + k),
            );
        }
    }
    counter(mb, pal);
    // Lanterns hang inside the two front posts, and three from a
    // ring round the pole.
    let mut lamps = Vec::with_capacity(5);
    for k in [7, 0] {
        let inward = (PAVILION - post(k)).normalize() * 0.25;
        lamps.push(lantern(
            mb,
            glass,
            pal,
            post(k) + inward + Vec3::Y * eave,
        ));
    }
    let hoop = PAVILION + Vec3::Y * (eave + 0.3);
    mb.torus(hoop, Vec3::Y, 0.55, 0.025, 16, pal.brass);
    for k in 0..3 {
        let a = k as f32 / 3.0 * TAU + 0.5;
        let from = hoop + Vec3::new(a.sin(), 0.0, a.cos()) * 0.55;
        lamps.push(lantern(mb, glass, pal, from - Vec3::Y * 0.2));
    }
    lamps
}

/// The pavilion's counter with what is for sale on it, and its
/// stock in the shade behind.
fn counter(mb: &mut Meshing, pal: &Palette) {
    let counter = PAVILION + Vec3::new(0.0, FLOOR, 2.2);
    mb.slab(
        counter + Vec3::Y * 0.45,
        Vec3::new(1.6, 0.45, 0.35),
        pal.timber,
    );
    mb.slab(
        counter + Vec3::Y * 0.92,
        Vec3::new(1.72, 0.04, 0.42),
        pal.timber_dark,
    );
    // A cloth over its front, fringed with brass.
    for (z, half, colour) in [
        (0.37, Vec3::new(1.5, 0.32, 0.01), pal.awnings[0]),
        (0.385, Vec3::new(1.3, 0.2, 0.01), pal.awnings[1]),
    ] {
        mb.rbox(
            counter + Vec3::new(0.0, 0.5, z),
            half,
            0.005,
            Quat::IDENTITY,
            colour,
            0.0,
        );
    }
    for k in 0..13 {
        let x = -1.44 + 0.24 * k as f32;
        mb.ball(
            counter + Vec3::new(x, 0.16, 0.39),
            Vec3::splat(0.03),
            2,
            pal.brass,
        );
    }
    let top = counter + Vec3::Y * 0.96;
    for (x, colour, size) in [
        (-1.35, pal.terracotta, 0.16),
        (-1.0, pal.brass, 0.12),
        (-0.72, pal.terracotta, 0.11),
    ] {
        mb.ball(
            top + Vec3::new(x, size, 0.0),
            Vec3::new(size, size * 1.15, size),
            4,
            colour,
        );
        mb.tube(
            top + Vec3::new(x, size * 2.1, 0.0),
            top + Vec3::new(x, size * 2.5, 0.0),
            size * 0.45,
            size * 0.55,
            8,
            colour,
            true,
        );
    }
    // A pair of scales, which is what a bazaar is for.
    let scales = top + Vec3::new(0.25, 0.0, -0.05);
    mb.tube(
        scales,
        scales + Vec3::Y * 0.55,
        0.025,
        0.02,
        6,
        pal.brass,
        true,
    );
    mb.rod(
        scales + Vec3::new(-0.32, 0.55, 0.0),
        scales + Vec3::new(0.32, 0.55, 0.0),
        0.015,
        4,
        pal.brass,
        true,
    );
    for side in [-1.0f32, 1.0] {
        let pan = scales + Vec3::new(side * 0.32, 0.25, 0.0);
        mb.rod(pan + Vec3::Y * 0.3, pan, 0.008, 3, pal.brass, false);
        mb.torus(pan, Vec3::Y, 0.11, 0.018, 10, pal.brass);
    }
    for (k, x) in [0.75f32, 0.95, 1.12].into_iter().enumerate() {
        let tall = 0.04 + 0.03 * k as f32;
        mb.rod(
            top + Vec3::new(x, 0.0, 0.08),
            top + Vec3::new(x, tall, 0.08),
            0.06,
            8,
            pal.brass,
            true,
        );
    }
    for (at, colour) in [
        (Vec3::new(-1.6, 0.0, -0.9), pal.ochre),
        (Vec3::new(-1.1, 0.0, -1.5), pal.parchment),
        (Vec3::new(1.5, 0.0, -1.2), pal.ochre),
    ] {
        mb.blob(
            PAVILION + at + Vec3::Y * (FLOOR + 0.32),
            Vec3::new(0.3, 0.34, 0.28),
            4,
            0.12,
            at.x,
            |_| colour,
        );
    }
    for (k, at) in
        [Vec3::new(0.6, 0.0, -1.7), Vec3::new(1.05, 0.0, -1.95)]
            .into_iter()
            .enumerate()
    {
        let half = 0.3 - 0.04 * k as f32;
        mb.rbox(
            PAVILION + at + Vec3::Y * (FLOOR + half),
            Vec3::splat(half),
            0.03,
            Quat::from_rotation_y(0.3 * k as f32),
            pal.timber,
            0.15,
        );
    }
}

/// The gate at the front: two posts, a beam with a sign of a coin
/// under it, and a lantern at each post. Returns the spots of its
/// lanterns.
pub(super) fn gate(
    mb: &mut Meshing,
    glass: &mut Meshing,
    pal: &Palette,
) -> [Vec3; 2] {
    let (wide, z, tall) = GATE;
    for side in [-1.0f32, 1.0] {
        let x = side * wide;
        mb.tube(
            Vec3::new(x, FLOOR, z),
            Vec3::new(x, tall, z),
            0.12,
            0.1,
            8,
            pal.timber_dark,
            true,
        );
        // Bands of brass up the post, and a knob at its top.
        for y in [0.6f32, 1.8] {
            mb.torus(
                Vec3::new(x, y, z),
                Vec3::Y,
                0.12,
                0.025,
                10,
                pal.brass,
            );
        }
        mb.ball(
            Vec3::new(x, tall + 0.08, z),
            Vec3::splat(0.14),
            3,
            pal.brass,
        );
        // A mast for a flag over each.
        mb.tube(
            Vec3::new(x, tall + 0.15, z),
            Vec3::new(x, tall + 0.8, z),
            0.03,
            0.02,
            5,
            pal.timber_dark,
            true,
        );
    }
    mb.slab(
        Vec3::new(0.0, tall - 0.15, z),
        Vec3::new(wide + 0.35, 0.1, 0.12),
        pal.timber_dark,
    );
    for side in [-1.0f32, 1.0] {
        mb.rod(
            Vec3::new(side * 0.75, tall - 0.25, z),
            Vec3::new(side * 0.75, tall - 0.55, z),
            0.015,
            4,
            pal.steel,
            false,
        );
    }
    let sign = Vec3::new(0.0, tall - 0.95, z);
    mb.slab(sign, Vec3::new(1.05, 0.4, 0.05), pal.awnings[0]);
    mb.slab(sign, Vec3::new(0.95, 0.31, 0.065), pal.whitewash);
    // A coin on each face of the sign.
    for out in [1.0f32, -1.0] {
        let coin = sign + Vec3::Z * out * 0.065;
        mb.rod(
            coin,
            coin + Vec3::Z * out * 0.04,
            0.26,
            16,
            pal.brass,
            true,
        );
        mb.torus(
            coin + Vec3::Z * out * 0.045,
            Vec3::Z,
            0.19,
            0.02,
            14,
            pal.ochre,
        );
    }
    [-1.0f32, 1.0].map(|side| {
        let arm = Vec3::new(side * (wide + 0.3), tall - 0.6, z);
        mb.rod(
            Vec3::new(side * wide, tall - 0.6, z),
            arm,
            0.03,
            4,
            pal.steel,
            false,
        );
        lantern(mb, glass, pal, arm)
    })
}
