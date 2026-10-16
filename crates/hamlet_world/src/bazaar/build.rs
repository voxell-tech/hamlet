//! The bazaar as meshes: a paved round with a striped pavilion in its
//! middle, five stalls in a ring about it facing out onto a
//! promenade, and a gate at its front, all hung with lanterns,
//! bunting and fairy lights.

mod props;
mod stall;

use std::f32::consts::{FRAC_PI_4, TAU};

use bevy::math::Affine3A;
use bevy::prelude::*;

#[cfg(test)]
pub(super) use self::props::PALMS;
pub(super) use self::props::{TEA, TEAPOT};
pub(super) use self::stall::{KEEPER, SHIMMER, SPARKS, at_work};
use super::stalls::Stall;
use crate::meshes::Mb;
use crate::palette::Palette;

/// The height of the paving.
pub(super) const FLOOR: f32 = 0.14;

/// The colours of the fairy lights, which take turns along a string.
pub(super) const FAIRY: usize = 6;

/// The middle of the pavilion, at the bazaar's own.
pub(super) const PAVILION: Vec3 = Vec3::ZERO;

/// The pavilion's posts stand on this radius, its canopy's rim is
/// this wide and this high, and its centre pole this tall.
pub(super) const POSTS: f32 = 3.2;
pub(super) const RIM: (f32, f32) = (3.7, 3.45);
pub(super) const POLE: f32 = 8.2;

/// The stripes of the pavilion's canopy, round it.
const STRIPES: usize = 16;

/// The light the far side of a cloth keeps.
const SHADE: f32 = 0.72;

/// The gate's posts either side of the way in, how far out they
/// stand and their height.
const GATE: (f32, f32, f32) = (2.3, 10.2, 3.8);

/// The radius of the paving.
pub(super) const PAVED: f32 = 10.8;

/// The radius the stalls stand on about the pavilion.
const RING: f32 = 6.9;

/// The room between two bulbs of a string of fairy lights.
const BULB_SPACING: f32 = 0.3;

/// Where `stall` stands and the way it turns: on the ring, a fifth
/// of the way round from the next, its back to the pavilion and its
/// front to the promenade. The gate is between the first and the
/// last.
pub(super) fn placing(stall: Stall) -> (Vec3, Quat) {
    let a =
        (stall as usize as f32 + 0.5) / Stall::ALL.len() as f32 * TAU;
    (
        Vec3::new(a.sin(), 0.0, a.cos()) * RING,
        Quat::from_rotation_y(a),
    )
}

/// The spot `local` of `stall`, from the bazaar's middle.
pub(super) fn at_stall(stall: Stall, local: Vec3) -> Vec3 {
    let (at, turn) = placing(stall);
    at + turn * local
}

/// A flag that waves: where its hoist's top hangs, its size and
/// the colours of its stripes.
#[derive(Clone, Copy, Debug)]
pub(super) struct Pennon {
    pub at: Vec3,
    pub long: f32,
    pub tall: f32,
    pub colours: [Vec3; 2],
}

/// A part that turns on its own: its mesh about an axle along its
/// Z, the axle's place and the way it lies, its turns a second and
/// the turn it starts at.
pub(super) struct Cog {
    pub mesh: Mesh,
    pub at: Vec3,
    pub lies: Quat,
    pub rate: f32,
    pub phase: f32,
}

/// The meshes of the bazaar, and the spots of what is drawn apart.
pub(super) struct Built {
    /// Its cloth, timber and wares.
    pub walls: Mesh,
    /// The lit glass of its lanterns.
    pub glass: Mesh,
    /// What glows among its wares, in colours of its own.
    pub gleam: Mesh,
    /// The bulbs of its fairy lights, a mesh to each colour.
    pub bulbs: [Mesh; FAIRY],
    pub lamps: Vec<Vec3>,
    pub flags: Vec<Pennon>,
    pub cogs: Vec<Cog>,
}

/// A cloth seen from both sides, its back in shade.
pub(super) fn cloth(
    mb: &mut Mb,
    p: [Vec3; 4],
    out: Vec3,
    colour: Vec3,
) {
    mb.face(p, out, colour);
    mb.face(p, -out, colour * SHADE);
}

/// The `i`th stripe of an awning of the colour `colour`, every
/// other one white.
pub(super) fn stripe(pal: &Palette, colour: usize, i: usize) -> Vec3 {
    if i.is_multiple_of(2) {
        pal.awnings[colour % pal.awnings.len()]
    } else {
        pal.whitewash
    }
}

/// A box of `colour` with its middle at `at`.
pub(super) fn slab(mb: &mut Mb, at: Vec3, half: Vec3, colour: Vec3) {
    mb.rbox(at, half, 0.03, Quat::IDENTITY, colour, 0.0);
}

/// A flat ring about `centre` from `inner` to `outer`, its
/// `segs` tiles coloured by `tile`.
fn ring(
    mb: &mut Mb,
    centre: Vec3,
    (inner, outer): (f32, f32),
    segs: usize,
    tile: impl Fn(usize) -> Vec3,
) {
    // From the outside in, so that it faces up.
    mb.lathe(centre, &[(outer, 0.0), (inner, 0.0)], segs, |_, s| {
        tile(s)
    });
}

/// A lantern hanging from `from`: a chain, a cap, a foot and lit
/// glass between. Returns the middle of its glass.
pub(super) fn lantern(
    mb: &mut Mb,
    glass: &mut Mb,
    pal: &Palette,
    from: Vec3,
) -> Vec3 {
    let middle = from - Vec3::Y * 0.42;
    mb.tube(
        from,
        middle + Vec3::Y * 0.2,
        0.012,
        0.012,
        4,
        pal.steel,
        false,
    );
    slab(
        mb,
        middle + Vec3::Y * 0.17,
        Vec3::new(0.13, 0.03, 0.13),
        pal.steel,
    );
    slab(
        mb,
        middle - Vec3::Y * 0.17,
        Vec3::new(0.11, 0.025, 0.11),
        pal.steel,
    );
    glass.rbox(
        middle,
        Vec3::new(0.1, 0.14, 0.1),
        0.03,
        Quat::IDENTITY,
        Vec3::ONE,
        0.0,
    );
    middle
}

/// The point `t` of the way along a line from `a` to `b` that sags
/// `sag` at its middle.
fn sagging((a, b): (Vec3, Vec3), sag: f32, t: f32) -> Vec3 {
    a.lerp(b, t) - Vec3::Y * sag * 4.0 * t * (1.0 - t)
}

/// A rope from `a` to `b` that sags `sag`, in `pieces`.
fn rope(
    mb: &mut Mb,
    (a, b): (Vec3, Vec3),
    sag: f32,
    pieces: usize,
    colour: Vec3,
) {
    let mut last = a;
    for k in 1..=pieces {
        let at = sagging((a, b), sag, k as f32 / pieces as f32);
        mb.tube(last, at, 0.01, 0.01, 4, colour, false);
        last = at;
    }
}

/// A string of pennants from `a` to `b` that sags `sag`.
fn bunting(
    mb: &mut Mb,
    pal: &Palette,
    line: (Vec3, Vec3),
    sag: f32,
    salt: usize,
) {
    let (a, b) = line;
    let along = (b - a).normalize_or(Vec3::X);
    let out = along.cross(Vec3::Y).normalize_or(Vec3::Z);
    let flags = ((b - a).length() / 0.45).floor().max(2.0) as usize;
    rope(mb, line, sag, flags * 2, pal.timber_dark);
    for k in 1..flags {
        let at = sagging(line, sag, k as f32 / flags as f32);
        let colour = match k % 2 {
            0 => pal.whitewash,
            _ => pal.awnings[(k + salt) % pal.awnings.len()],
        };
        cloth(
            mb,
            [
                at - along * 0.14,
                at + along * 0.14,
                at - Vec3::Y * 0.3,
                at - Vec3::Y * 0.3,
            ],
            out,
            colour,
        );
    }
}

/// Strings of fairy lights, the bulbs of each colour in a mesh of
/// their own.
struct Fairy {
    bulbs: [Mb; FAIRY],
    /// Bulbs hung so far, which tells the colour of the next.
    hung: usize,
}

impl Fairy {
    fn new() -> Self {
        Fairy {
            bulbs: std::array::from_fn(|_| Mb::new()),
            hung: 0,
        }
    }

    /// A string from `a` to `b` that sags `sag`: its wire and
    /// sockets in `mb`, and a bulb under each socket.
    fn string(
        &mut self,
        mb: &mut Mb,
        pal: &Palette,
        line: (Vec3, Vec3),
        sag: f32,
    ) {
        let long = (line.1 - line.0).length();
        let bulbs = (long / BULB_SPACING).round().max(2.0) as usize;
        rope(mb, line, sag, bulbs, pal.ink);
        for k in 0..bulbs {
            let at =
                sagging(line, sag, (k as f32 + 0.5) / bulbs as f32);
            mb.tube(
                at,
                at - Vec3::Y * 0.04,
                0.017,
                0.017,
                4,
                pal.ink,
                true,
            );
            self.bulbs[self.hung % FAIRY].blob(
                at - Vec3::Y * 0.085,
                Vec3::new(0.045, 0.062, 0.045),
                3,
                0.0,
                0.0,
                |_| Vec3::ONE,
            );
            self.hung += 1;
        }
    }
}

/// The paving: a border of tiles, tiles round the pavilion's foot,
/// a star in the plaza before it, stepping stones from the gate, and
/// a rug in the pavilion.
fn ground(mb: &mut Mb, pal: &Palette) {
    mb.tube(
        Vec3::ZERO,
        Vec3::Y * 0.1,
        PAVED + 0.45,
        PAVED + 0.45,
        48,
        pal.limestone_shade,
        true,
    );
    mb.tube(
        Vec3::ZERO,
        Vec3::Y * FLOOR,
        PAVED,
        PAVED,
        48,
        pal.dirt,
        true,
    );
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
        mb.rbox(
            star + Vec3::Y * lift,
            Vec3::new(0.6, 0.004, 0.6),
            0.0,
            Quat::from_rotation_y(turn),
            colour,
            0.0,
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
    slab(
        mb,
        rug + Vec3::Y * (FLOOR + 0.006),
        Vec3::new(2.7, 0.008, 2.1),
        pal.awnings[1],
    );
    slab(
        mb,
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
fn rim_at(s: usize, out: f32, below: f32) -> Vec3 {
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
fn pavilion(mb: &mut Mb, glass: &mut Mb, pal: &Palette) -> Vec<Vec3> {
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
        mb.blob(
            post(k) + Vec3::Y * (eave + 0.12),
            Vec3::splat(0.1),
            3,
            0.0,
            0.0,
            |_| pal.brass,
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
    let under = canopy
        .iter()
        .rev()
        .map(|(r, y)| (r * 0.97, y - 0.05))
        .collect::<Vec<_>>();
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
        cloth(mb, [left, right, tip, tip], out, colour);
        mb.blob(tip, Vec3::splat(0.035), 2, 0.0, 0.0, |_| pal.brass);
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
    mb.blob(
        PAVILION + Vec3::Y * (eave + 4.1),
        Vec3::splat(0.13),
        4,
        0.0,
        0.0,
        |_| pal.brass,
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
            cloth(
                mb,
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
    let mut lamps = [7, 0]
        .into_iter()
        .map(|k| {
            let inward = (PAVILION - post(k)).normalize() * 0.25;
            lantern(mb, glass, pal, post(k) + inward + Vec3::Y * eave)
        })
        .collect::<Vec<_>>();
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
fn counter(mb: &mut Mb, pal: &Palette) {
    let counter = PAVILION + Vec3::new(0.0, FLOOR, 2.2);
    slab(
        mb,
        counter + Vec3::Y * 0.45,
        Vec3::new(1.6, 0.45, 0.35),
        pal.timber,
    );
    slab(
        mb,
        counter + Vec3::Y * 0.92,
        Vec3::new(1.72, 0.04, 0.42),
        pal.timber_dark,
    );
    // A cloth over its front, fringed with brass.
    mb.rbox(
        counter + Vec3::new(0.0, 0.5, 0.37),
        Vec3::new(1.5, 0.32, 0.01),
        0.005,
        Quat::IDENTITY,
        pal.awnings[0],
        0.0,
    );
    mb.rbox(
        counter + Vec3::new(0.0, 0.5, 0.385),
        Vec3::new(1.3, 0.2, 0.01),
        0.005,
        Quat::IDENTITY,
        pal.awnings[1],
        0.0,
    );
    for k in 0..13 {
        let x = -1.44 + 0.24 * k as f32;
        mb.blob(
            counter + Vec3::new(x, 0.16, 0.39),
            Vec3::splat(0.03),
            2,
            0.0,
            0.0,
            |_| pal.brass,
        );
    }
    let top = counter + Vec3::Y * 0.96;
    for (x, colour, size) in [
        (-1.35, pal.terracotta, 0.16),
        (-1.0, pal.brass, 0.12),
        (-0.72, pal.terracotta, 0.11),
    ] {
        mb.blob(
            top + Vec3::new(x, size, 0.0),
            Vec3::new(size, size * 1.15, size),
            4,
            0.0,
            0.0,
            |_| colour,
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
    mb.tube(
        scales + Vec3::new(-0.32, 0.55, 0.0),
        scales + Vec3::new(0.32, 0.55, 0.0),
        0.015,
        0.015,
        4,
        pal.brass,
        true,
    );
    for side in [-1.0f32, 1.0] {
        let pan = scales + Vec3::new(side * 0.32, 0.25, 0.0);
        mb.tube(
            pan + Vec3::Y * 0.3,
            pan,
            0.008,
            0.008,
            3,
            pal.brass,
            false,
        );
        mb.torus(pan, Vec3::Y, 0.11, 0.018, 10, pal.brass);
    }
    for (k, x) in [0.75f32, 0.95, 1.12].into_iter().enumerate() {
        let tall = 0.04 + 0.03 * k as f32;
        mb.tube(
            top + Vec3::new(x, 0.0, 0.08),
            top + Vec3::new(x, tall, 0.08),
            0.06,
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
fn gate(mb: &mut Mb, glass: &mut Mb, pal: &Palette) -> Vec<Vec3> {
    let (wide, z, tall) = GATE;
    for side in [-1.0f32, 1.0] {
        mb.tube(
            Vec3::new(side * wide, FLOOR, z),
            Vec3::new(side * wide, tall, z),
            0.12,
            0.1,
            8,
            pal.timber_dark,
            true,
        );
        // Bands of brass up the post, and a knob at its top.
        for y in [0.6f32, 1.8] {
            mb.torus(
                Vec3::new(side * wide, y, z),
                Vec3::Y,
                0.12,
                0.025,
                10,
                pal.brass,
            );
        }
        mb.blob(
            Vec3::new(side * wide, tall + 0.08, z),
            Vec3::splat(0.14),
            3,
            0.0,
            0.0,
            |_| pal.brass,
        );
        // A mast for a flag over each.
        mb.tube(
            Vec3::new(side * wide, tall + 0.15, z),
            Vec3::new(side * wide, tall + 0.8, z),
            0.03,
            0.02,
            5,
            pal.timber_dark,
            true,
        );
    }
    slab(
        mb,
        Vec3::new(0.0, tall - 0.15, z),
        Vec3::new(wide + 0.35, 0.1, 0.12),
        pal.timber_dark,
    );
    for side in [-1.0f32, 1.0] {
        mb.tube(
            Vec3::new(side * 0.75, tall - 0.25, z),
            Vec3::new(side * 0.75, tall - 0.55, z),
            0.015,
            0.015,
            4,
            pal.steel,
            false,
        );
    }
    let sign = Vec3::new(0.0, tall - 0.95, z);
    slab(mb, sign, Vec3::new(1.05, 0.4, 0.05), pal.awnings[0]);
    slab(mb, sign, Vec3::new(0.95, 0.31, 0.065), pal.whitewash);
    // A coin on each face of the sign.
    for out in [1.0f32, -1.0] {
        let coin = sign + Vec3::Z * out * 0.065;
        mb.tube(
            coin,
            coin + Vec3::Z * out * 0.04,
            0.26,
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
    [-1.0f32, 1.0]
        .into_iter()
        .map(|side| {
            let arm = Vec3::new(side * (wide + 0.3), tall - 0.6, z);
            mb.tube(
                Vec3::new(side * wide, tall - 0.6, z),
                arm,
                0.03,
                0.03,
                4,
                pal.steel,
                false,
            );
            lantern(mb, glass, pal, arm)
        })
        .collect()
}

/// The bazaar's cloth, timber and wares, the lit glass of its
/// lanterns, what glows among its wares and the bulbs of its fairy
/// lights apart from them, the spots of its lanterns and flags, and
/// The bazaar as a few plain shapes, for when it is seen from far
/// away: its paving, the pavilion's canopy and a block for each stall
/// in its cloth.
pub(super) fn far(pal: &Palette) -> Mesh {
    let mut mb = Mb::new();
    mb.tube(
        Vec3::ZERO,
        Vec3::Y * FLOOR,
        PAVED + 0.45,
        PAVED + 0.45,
        16,
        pal.dirt,
        true,
    );
    let (rim, eave) = RIM;
    mb.tube(
        PAVILION + Vec3::Y * FLOOR,
        PAVILION + Vec3::Y * eave,
        POSTS,
        POSTS,
        8,
        pal.awnings[2],
        false,
    );
    mb.lathe(
        PAVILION + Vec3::Y * eave,
        &[(rim, 0.0), (0.0, 3.4)],
        8,
        |_, s| stripe(pal, 0, s),
    );
    for (k, stall) in Stall::ALL.into_iter().enumerate() {
        let (at, turn) = placing(stall);
        let cloth = pal.stalls[k % pal.stalls.len()].cloth;
        mb.rbox(
            at + Vec3::Y * 1.4,
            Vec3::new(1.6, 1.3, 1.0),
            0.05,
            turn,
            cloth,
            0.2,
        );
    }
    mb.build()
}

/// the parts that turn.
pub(super) fn build(pal: &Palette) -> Built {
    let (mut mb, mut glass, mut gleam) =
        (Mb::new(), Mb::new(), Mb::new());
    let mut fairy = Fairy::new();
    ground(&mut mb, pal);
    let mut lamps = pavilion(&mut mb, &mut glass, pal);
    let mut flags = vec![Pennon {
        at: PAVILION + Vec3::Y * (POLE - 0.02),
        long: 1.3,
        tall: 0.65,
        colours: [pal.awnings[1], pal.awnings[0]],
    }];
    let mut cogs = Vec::new();
    // Where each stall's strings are tied: the top of its sign, and
    // the two ends of its front.
    let mut ties = Vec::new();
    for stall in Stall::ALL {
        let (at, turn) = placing(stall);
        let xf = Affine3A::from_rotation_translation(turn, at);
        for part in [&mut mb, &mut glass, &mut gleam] {
            part.at(xf);
        }
        let dressed = stall::stall(
            &mut mb,
            (&mut glass, &mut gleam),
            pal,
            stall,
        );
        let to = |p: Vec3| xf.transform_point3(p);
        lamps.push(to(dressed.lamp));
        let style = pal.stalls[stall as usize];
        flags.push(Pennon {
            at: to(dressed.mast),
            long: 0.6,
            tall: 0.32,
            colours: [style.cloth, style.trim],
        });
        ties.push((to(dressed.crest), dressed.ties.map(to)));
        cogs.extend(dressed.cogs.into_iter().map(|cog| Cog {
            at: to(cog.at),
            lies: turn * cog.lies,
            ..cog
        }));
    }
    for part in [&mut mb, &mut glass, &mut gleam] {
        part.at(Affine3A::IDENTITY);
    }
    lamps.extend(gate(&mut mb, &mut glass, pal));
    let (wide, z, tall) = GATE;
    for side in [-1.0f32, 1.0] {
        flags.push(Pennon {
            at: Vec3::new(side * wide, tall + 0.78, z),
            long: 0.8,
            tall: 0.4,
            colours: [pal.awnings[2], pal.awnings[1]],
        });
    }
    props::furnish(&mut mb, pal);

    // Bunting from the top of the pole to the gate, and fairy lights
    // from it to every stall's sign, round the pavilion's rim, along
    // each stall's front, from stall to stall round the ring, from
    // the gate to the stalls beside it and under the gate's beam.
    let top = PAVILION + Vec3::Y * (POLE - 0.3);
    let gates = [-1.0f32, 1.0]
        .map(|side| Vec3::new(side * wide, tall - 0.08, z));
    for (k, gate) in gates.into_iter().enumerate() {
        bunting(&mut mb, pal, (top, gate), 0.45, k);
    }
    for (crest, front) in &ties {
        fairy.string(&mut mb, pal, (top, *crest), 0.55);
        fairy.string(&mut mb, pal, (front[0], front[1]), 0.1);
    }
    for s in 0..STRIPES {
        fairy.string(
            &mut mb,
            pal,
            (rim_at(s, 0.05, 0.47), rim_at(s + 1, 0.05, 0.47)),
            0.1,
        );
    }
    // The nearest ends of two neighbours' fronts. The gate is between
    // the last and the first.
    let nearest = |a: [Vec3; 2], b: [Vec3; 2]| {
        let pairs =
            [(a[0], b[0]), (a[0], b[1]), (a[1], b[0]), (a[1], b[1])];
        pairs
            .into_iter()
            .min_by(|p, q| {
                p.0.distance(p.1).total_cmp(&q.0.distance(q.1))
            })
            .unwrap_or(pairs[0])
    };
    for pair in ties.windows(2) {
        let (a, b) = nearest(pair[0].1, pair[1].1);
        fairy.string(&mut mb, pal, (a, b), 0.35);
    }
    for (gate, (_, front)) in
        gates.into_iter().zip([&ties[ties.len() - 1], &ties[0]])
    {
        let (_, end) = nearest([gate; 2], *front);
        fairy.string(&mut mb, pal, (gate, end), 0.3);
    }
    fairy.string(
        &mut mb,
        pal,
        (
            Vec3::new(-wide - 0.3, tall - 0.27, z + 0.16),
            Vec3::new(wide + 0.3, tall - 0.27, z + 0.16),
        ),
        0.22,
    );
    Built {
        walls: mb.build(),
        glass: glass.build(),
        gleam: gleam.build(),
        bulbs: fairy.bulbs.map(Mb::build),
        lamps,
        flags,
        cogs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bazaar::{GROUNDS, SIZE};

    #[test]
    fn the_bazaar_stands_within_its_ground() {
        let built = build(&Palette::default());
        let cogs = built.cogs.into_iter().map(|cog| {
            let xf =
                Affine3A::from_rotation_translation(cog.lies, cog.at);
            (cog.mesh, xf)
        });
        let meshes = [built.walls, built.glass, built.gleam]
            .into_iter()
            .chain(built.bulbs)
            .map(|mesh| (mesh, Affine3A::IDENTITY))
            .chain(cogs);
        for (mesh, xf) in meshes {
            let points = mesh
                .attribute(Mesh::ATTRIBUTE_POSITION)
                .and_then(|points| points.as_float3())
                .unwrap();
            assert!(points.len() > 3);
            // It stands on the ground and within what it is
            // picked by.
            for point in points {
                let [x, y, z] = xf
                    .transform_point3(Vec3::from(*point))
                    .to_array();
                assert!((-0.01..=SIZE.1).contains(&y), "{y}");
                assert!(x.hypot(z) <= SIZE.0, "{x} {z}");
            }
        }
        assert!(SIZE.0 < GROUNDS);
        // Five in the pavilion, one to each stall and one at each
        // gate post.
        assert_eq!(built.lamps.len(), 12);
        // On the pole, a stall's mast each and on each gate post.
        assert_eq!(built.flags.len(), 8);
    }
}
