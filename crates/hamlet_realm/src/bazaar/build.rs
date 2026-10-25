//! The bazaar as meshes: a paved round, a pavilion, five stalls in a
//! ring facing a promenade, and a gate, hung with lanterns, bunting
//! and fairy lights.

mod pavilion;
mod props;
mod stall;

use std::f32::consts::TAU;

use bevy::math::Affine3A;
use bevy::prelude::*;

#[cfg(test)]
pub(super) use self::props::PALMS;
pub(super) use self::props::{TEA, TEAPOT};
pub(super) use self::stall::{KEEPER, SHIMMER, SPARKS, at_work};
use super::Stall;
use crate::meshes::Meshing;
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
const PAVED: f32 = 10.8;

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
/// Z, the axle's place and the way it lies, and its turns a second.
pub(super) struct Cog {
    pub mesh: Mesh,
    pub at: Vec3,
    pub lies: Quat,
    pub rate: f32,
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

/// The few shapes most of the bazaar is made of, over [`Meshing`]'s
/// own.
pub(super) trait Shapes {
    /// A smooth ball of one colour.
    fn ball(&mut self, at: Vec3, radii: Vec3, rings: usize, c: Vec3);
    /// A bar of one radius from `a` to `b`.
    fn rod(
        &mut self,
        a: Vec3,
        b: Vec3,
        radius: f32,
        segs: usize,
        c: Vec3,
        caps: bool,
    );
    /// A box with sharp edges, turned `turn`.
    fn plate(&mut self, at: Vec3, half: Vec3, turn: Quat, c: Vec3);
    /// A box with softened edges and its middle at `at`.
    fn slab(&mut self, at: Vec3, half: Vec3, c: Vec3);
    /// A cloth seen from both sides, its back in shade.
    fn cloth(&mut self, p: [Vec3; 4], out: Vec3, c: Vec3);
    /// A three-cornered cloth from one end of `edge` to the other
    /// down to `tip`.
    fn flap(
        &mut self,
        edge: (Vec3, Vec3),
        tip: Vec3,
        out: Vec3,
        c: Vec3,
    );
}

impl Shapes for Meshing {
    fn ball(&mut self, at: Vec3, radii: Vec3, rings: usize, c: Vec3) {
        self.blob(at, radii, rings, 0.0, 0.0, |_| c);
    }

    fn rod(
        &mut self,
        a: Vec3,
        b: Vec3,
        radius: f32,
        segs: usize,
        c: Vec3,
        caps: bool,
    ) {
        self.tube(a, b, radius, radius, segs, c, caps);
    }

    fn plate(&mut self, at: Vec3, half: Vec3, turn: Quat, c: Vec3) {
        self.rbox(at, half, 0.0, turn, c, 0.0);
    }

    fn slab(&mut self, at: Vec3, half: Vec3, c: Vec3) {
        self.rbox(at, half, 0.03, Quat::IDENTITY, c, 0.0);
    }

    fn cloth(&mut self, p: [Vec3; 4], out: Vec3, c: Vec3) {
        self.face(p, out, c);
        self.face(p, -out, c * SHADE);
    }

    fn flap(
        &mut self,
        (a, b): (Vec3, Vec3),
        tip: Vec3,
        out: Vec3,
        c: Vec3,
    ) {
        self.cloth([a, b, tip, tip], out, c);
    }
}

/// Builds `part` at `local` within what `mb` builds at already.
pub(super) fn within(
    mb: &mut Meshing,
    local: Affine3A,
    part: impl FnOnce(&mut Meshing),
) {
    let outer = mb.transform;
    mb.at(outer * local);
    part(mb);
    mb.at(outer);
}

/// The `i`th stripe of an awning of the colour `colour`, every
/// other one white.
fn stripe(pal: &Palette, colour: usize, i: usize) -> Vec3 {
    if i.is_multiple_of(2) {
        pal.awnings[colour % pal.awnings.len()]
    } else {
        pal.whitewash
    }
}

/// A flat ring about `centre` from `inner` to `outer`, its `segs`
/// tiles coloured by `tile`.
fn ring(
    mb: &mut Meshing,
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
    mb: &mut Meshing,
    glass: &mut Meshing,
    pal: &Palette,
    from: Vec3,
) -> Vec3 {
    let middle = from - Vec3::Y * 0.42;
    mb.rod(from, middle + Vec3::Y * 0.2, 0.012, 4, pal.steel, false);
    mb.slab(
        middle + Vec3::Y * 0.17,
        Vec3::new(0.13, 0.03, 0.13),
        pal.steel,
    );
    mb.slab(
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
    mb: &mut Meshing,
    line: (Vec3, Vec3),
    sag: f32,
    pieces: usize,
    colour: Vec3,
) {
    let mut last = line.0;
    for k in 1..=pieces {
        let at = sagging(line, sag, k as f32 / pieces as f32);
        mb.rod(last, at, 0.01, 4, colour, false);
        last = at;
    }
}

/// A string of pennants from `a` to `b` that sags `sag`.
fn bunting(
    mb: &mut Meshing,
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
        mb.flap(
            (at - along * 0.14, at + along * 0.14),
            at - Vec3::Y * 0.3,
            out,
            colour,
        );
    }
}

/// Strings of fairy lights, the bulbs of each colour in a mesh of
/// their own.
struct Fairy {
    bulbs: [Meshing; FAIRY],
    /// Bulbs hung so far, which tells the colour of the next.
    hung: usize,
}

impl Fairy {
    fn new() -> Self {
        Fairy {
            bulbs: std::array::from_fn(|_| Meshing::new()),
            hung: 0,
        }
    }

    /// A string from `a` to `b` that sags `sag`: its wire and
    /// sockets in `mb`, and a bulb under each socket.
    fn string(
        &mut self,
        mb: &mut Meshing,
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
            mb.rod(at, at - Vec3::Y * 0.04, 0.017, 4, pal.ink, true);
            self.bulbs[self.hung % FAIRY].ball(
                at - Vec3::Y * 0.085,
                Vec3::new(0.045, 0.062, 0.045),
                3,
                Vec3::ONE,
            );
            self.hung += 1;
        }
    }
}

/// The nearest ends of two neighbours' fronts.
fn nearest(a: [Vec3; 2], b: [Vec3; 2]) -> (Vec3, Vec3) {
    let pairs =
        [(a[0], b[0]), (a[0], b[1]), (a[1], b[0]), (a[1], b[1])];
    pairs
        .into_iter()
        .min_by(|p, q| {
            p.0.distance(p.1).total_cmp(&q.0.distance(q.1))
        })
        .unwrap_or(pairs[0])
}

/// The bazaar as a few plain shapes, for when it is seen from far
/// away: its paving, the pavilion's canopy and a block for each stall
/// in its cloth.
pub(super) fn far(pal: &Palette) -> Mesh {
    let mut mb = Meshing::new();
    mb.rod(
        Vec3::ZERO,
        Vec3::Y * FLOOR,
        PAVED + 0.45,
        16,
        pal.dirt,
        true,
    );
    let (rim, eave) = RIM;
    mb.rod(
        PAVILION + Vec3::Y * FLOOR,
        PAVILION + Vec3::Y * eave,
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
    for stall in Stall::ALL {
        let (at, turn) = placing(stall);
        mb.rbox(
            at + Vec3::Y * 1.4,
            Vec3::new(1.6, 1.3, 1.0),
            0.05,
            turn,
            pal.stalls[stall as usize].cloth,
            0.2,
        );
    }
    mb.build()
}

/// The bazaar's cloth, timber and wares, the lit glass of its
/// lanterns, what glows among its wares and the bulbs of its fairy
/// lights apart from them, the spots of its lanterns and flags, and
/// the parts that turn.
pub(super) fn build(pal: &Palette) -> Built {
    let (mut mb, mut glass, mut gleam) =
        (Meshing::new(), Meshing::new(), Meshing::new());
    let mut fairy = Fairy::new();
    pavilion::ground(&mut mb, pal);
    let mut lamps = pavilion::pavilion(&mut mb, &mut glass, pal);
    let mut flags = vec![Pennon {
        at: PAVILION + Vec3::Y * (POLE - 0.02),
        long: 1.3,
        tall: 0.65,
        colours: [pal.awnings[1], pal.awnings[0]],
    }];
    let mut cogs = Vec::new();
    // Where each stall's strings are tied: the top of its sign, and
    // the two ends of its front.
    let mut ties = Vec::with_capacity(Stall::ALL.len());
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
    lamps.extend(pavilion::gate(&mut mb, &mut glass, pal));
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
            (
                pavilion::rim_at(s, 0.05, 0.47),
                pavilion::rim_at(s + 1, 0.05, 0.47),
            ),
            0.1,
        );
    }
    // The gate is between the last and the first.
    for pair in ties.windows(2) {
        let line = nearest(pair[0].1, pair[1].1);
        fairy.string(&mut mb, pal, line, 0.35);
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
        bulbs: fairy.bulbs.map(Meshing::build),
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
