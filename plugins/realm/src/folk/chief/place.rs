//! The Hearth, a chief's place: a round house in a walled garden, as
//! meshes in the place's own frame, whose +Z is the way the door
//! faces.

use std::f32::consts::{FRAC_PI_4, TAU};

use glam::{Quat, Vec2, Vec3};

use crate::build::mesh::{Mb, Rng};
use crate::folk::chief::ChiefSettings;
use crate::folk::chief::look::Look;
use crate::places::lantern::lantern_cage;
use crate::world::land::Land;
use crate::world::palette::Palette;

/// The site of a chief's place, clear of the middles in `taken`: the
/// last neighbour of the first town a new town would be founded on.
/// None when the land has no room left.
pub fn stand(land: &Land, taken: &[Vec2]) -> Option<Vec2> {
    let spacing = land.set.land.site_spacing;
    let first = land.sites.iter().position(|site| site.buildable)?;
    let home = land.sites[first].at;
    let free = |at: Vec2| {
        taken.iter().all(|other| other.distance(at) > spacing * 0.5)
    };
    let open = land
        .sites
        .iter()
        .enumerate()
        .filter(|(i, site)| {
            *i != first && site.buildable && free(site.at)
        })
        .map(|(_, site)| site.at)
        .collect::<Vec<_>>();
    open.iter()
        .rev()
        .find(|at| at.distance(home) < spacing * 1.5)
        .or_else(|| {
            open.iter().min_by(|a, b| {
                a.distance(home).total_cmp(&b.distance(home))
            })
        })
        .copied()
}

/// The mascot's pastime at a spot of its grounds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pastime {
    /// Stands on the waystone.
    Wait,
    /// Reads a letter at the letterbox.
    Read,
    /// Waters the flower bed.
    Tend,
    /// Looks at the sky from beside the bench.
    Gaze,
}

/// A spot of the grounds the mascot stands at, and the point it
/// faces there.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Spot {
    pub at: Vec2,
    pub faces: Vec2,
    pub pastime: Pastime,
}

/// The spot of each thing of the grounds.
struct Plan {
    /// The radius of the wall.
    wall: f32,
    house: Vec2,
    waystone: Vec2,
    letterbox: Vec2,
    bed: Vec2,
    bench: Vec2,
}

impl Plan {
    fn of(set: &ChiefSettings) -> Self {
        let r = set.grounds;
        Self {
            wall: r,
            house: Vec2::new(0.0, -0.18) * r,
            waystone: Vec2::new(0.0, 0.5) * r,
            letterbox: Vec2::new(0.17, 0.8) * r,
            bed: Vec2::new(-0.55, 0.2) * r,
            bench: Vec2::new(0.56, 0.1) * r,
        }
    }
}

/// The spots of the grounds. The first is the waystone, where the
/// mascot leaves from and comes home to.
pub fn spots(set: &ChiefSettings) -> [Spot; 4] {
    let plan = Plan::of(set);
    [
        Spot {
            at: plan.waystone,
            faces: plan.waystone + Vec2::Y,
            pastime: Pastime::Wait,
        },
        Spot {
            at: plan.letterbox - Vec2::X * 0.7,
            faces: plan.letterbox,
            pastime: Pastime::Read,
        },
        Spot {
            at: plan.bed + Vec2::new(1.25, 0.35),
            faces: plan.bed,
            pastime: Pastime::Tend,
        },
        Spot {
            at: plan.bench + Vec2::new(-0.9, 0.75),
            faces: plan.bench + Vec2::new(-4.0, 6.0),
            pastime: Pastime::Gaze,
        },
    ]
}

/// The middle of the house.
pub fn house_at(set: &ChiefSettings) -> Vec2 {
    Plan::of(set).house
}

/// The lights of a place, each where its middle is.
pub struct Lights {
    /// The orb on the tip of the roof, and its radius.
    pub orb: (Vec3, f32),
    /// The glass of the lantern over the gate.
    pub lantern: Vec3,
    /// The ring let into the waystone, and its radius.
    pub ring: (Vec3, f32),
}

/// A tree of the grounds: where it stands, its kind and its size.
pub struct Planted {
    pub at: Vec3,
    pub kind: usize,
    pub scale: f32,
}

/// The meshes of a place and what is spawned beside them.
pub struct Built {
    pub body: Mb,
    pub panes: Mb,
    pub lights: Lights,
    pub trees: [Planted; 3],
}

fn up(at: Vec2, y: f32) -> Vec3 {
    Vec3::new(at.x, y, at.y)
}

/// The round house: a plinth, a plump wall with a door and two
/// round windows, a chimney and the hat of a roof with its bent tip.
fn house(
    mb: &mut Mb,
    panes: &mut Mb,
    pal: &Palette,
    base: Vec3,
    roof: Vec3,
) -> (Vec3, f32) {
    let (stone, shade) = (pal.limestone, pal.limestone_shade);
    mb.lathe(
        base,
        &[(2.5, -0.3), (2.5, 0.2), (2.36, 0.3), (2.1, 0.3)],
        24,
        |r, s| stone.lerp(shade, ((r * 5 + s * 3) % 4) as f32 * 0.14),
    );
    mb.lathe(
        base,
        &[(2.12, 0.3), (2.28, 1.05), (2.26, 2.1), (2.08, 3.0)],
        24,
        |r, s| {
            pal.plaster.lerp(
                pal.plaster_worn,
                ((r * 3 + s * 7) % 5) as f32 * 0.05,
            )
        },
    );
    for (y, big) in [(0.36, 2.15), (2.92, 2.1)] {
        mb.torus(
            base + Vec3::Y * y,
            Vec3::Y,
            big,
            0.07,
            24,
            pal.timber,
        );
    }

    // The hat: a dark underside, then rows of shingles that sweep in
    // to the point.
    let hat = [
        (2.05, 2.95),
        (2.95, 2.66),
        (3.0, 2.76),
        (2.62, 3.06),
        (2.1, 3.48),
        (1.55, 4.02),
        (1.06, 4.68),
        (0.66, 5.36),
        (0.4, 5.9),
        (0.26, 6.25),
    ];
    mb.lathe(base, &hat, 24, |r, s| {
        if r == 0 {
            return pal.timber_dark;
        }
        let row = if r % 2 == 0 { 1.0 } else { 0.84 };
        roof * row * (1.0 - ((r * 5 + s * 3) % 4) as f32 * 0.035)
    });
    let tip = base + Vec3::new(0.46, 6.85, 0.14);
    mb.tube(
        base + Vec3::Y * 6.19,
        tip,
        0.27,
        0.11,
        10,
        roof * 0.8,
        true,
    );
    mb.lathe(
        tip - Vec3::Y * 0.02,
        &[(0.0, 0.0), (0.15, 0.0), (0.2, 0.08), (0.17, 0.1)],
        10,
        |_, _| pal.brass,
    );

    let foot = base + Vec3::new(-1.35, 3.6, -0.9);
    let top = base + Vec3::new(-1.62, 5.4, -1.06);
    mb.tube(foot, top, 0.36, 0.3, 8, stone * 0.92, true);
    mb.rbox(
        top,
        Vec3::new(0.38, 0.07, 0.38),
        0.03,
        Quat::IDENTITY,
        shade,
        0.1,
    );

    // The door, round at the top, with a brass slot for letters.
    let front = base + Vec3::Z * 2.2;
    mb.rbox(
        front + Vec3::Y * 1.1,
        Vec3::new(0.62, 0.8, 0.16),
        0.04,
        Quat::IDENTITY,
        shade,
        0.2,
    );
    mb.tube(
        front + Vec3::new(0.0, 1.9, -0.16),
        front + Vec3::new(0.0, 1.9, 0.16),
        0.62,
        0.62,
        14,
        shade,
        true,
    );
    mb.rbox(
        front + Vec3::new(0.0, 1.07, 0.13),
        Vec3::new(0.47, 0.77, 0.06),
        0.02,
        Quat::IDENTITY,
        pal.timber,
        0.2,
    );
    mb.tube(
        front + Vec3::new(0.0, 1.84, 0.07),
        front + Vec3::new(0.0, 1.84, 0.19),
        0.47,
        0.47,
        14,
        pal.timber,
        true,
    );
    mb.rbox(
        front + Vec3::new(0.0, 1.3, 0.2),
        Vec3::new(0.17, 0.035, 0.012),
        0.006,
        Quat::IDENTITY,
        pal.brass,
        0.0,
    );
    mb.blob(
        front + Vec3::new(0.32, 1.0, 0.22),
        Vec3::splat(0.05),
        4,
        0.0,
        0.0,
        |_| pal.brass,
    );
    mb.rbox(
        front + Vec3::new(0.0, 0.14, 0.5),
        Vec3::new(0.75, 0.13, 0.36),
        0.04,
        Quat::IDENTITY,
        stone,
        0.3,
    );

    // Two round windows, either side of the door.
    for side in [-1.0f32, 1.0] {
        let a = side * 0.74;
        let out = Vec3::new(a.sin(), 0.0, a.cos());
        let across = Vec3::new(a.cos(), 0.0, -a.sin());
        let c = base + out * 2.22 + Vec3::Y * 1.85;
        mb.torus(c + out * 0.08, out, 0.46, 0.075, 16, pal.timber);
        panes.tube(
            c - out * 0.1,
            c + out * 0.07,
            0.46,
            0.46,
            16,
            Vec3::ONE,
            true,
        );
        for bar in [across, Vec3::Y] {
            mb.tube(
                c + out * 0.09 - bar * 0.45,
                c + out * 0.09 + bar * 0.45,
                0.025,
                0.025,
                5,
                pal.timber,
                false,
            );
        }
    }
    (tip + Vec3::Y * 0.26, 0.2)
}

/// The wall of loose stones, open at the gate.
fn wall(
    mb: &mut Mb,
    rng: &mut Rng,
    pal: &Palette,
    radius: f32,
    ground: &impl Fn(Vec2) -> f32,
) {
    let count = (TAU * radius / 0.56) as usize;
    let gate = 1.45 / radius;
    for i in 0..count {
        let a = (i as f32 + rng.sym(0.12)) / count as f32 * TAU;
        let off = a.min(TAU - a);
        if off < gate {
            continue;
        }
        let at =
            Vec2::new(a.sin(), a.cos()) * (radius + rng.sym(0.06));
        let tall = if rng.f() < 0.15 { 0.32 } else { 0.22 };
        let half = Vec3::new(
            rng.range(0.24, 0.3),
            tall + rng.sym(0.03),
            rng.range(0.16, 0.2),
        );
        mb.rbox(
            up(at, ground(at) + half.y - 0.04),
            half,
            0.05,
            Quat::from_rotation_y(a) * rng.wobble(3.0),
            pal.rock.lerp(pal.rock_warm, rng.f()) * rng.value(0.07),
            0.25,
        );
    }
}

/// The gate: two posts, a beam under a little roof, and the cage of
/// the lantern that hangs from it. Returns where its glass is.
fn gate(
    mb: &mut Mb,
    rng: &mut Rng,
    pal: &Palette,
    at: Vec3,
    roof: Vec3,
) -> Vec3 {
    for side in [-1.0f32, 1.0] {
        mb.rbox(
            at + Vec3::new(side * 1.2, 1.0, 0.0),
            Vec3::new(0.1, 1.0, 0.1),
            0.03,
            rng.wobble(1.5),
            pal.timber * rng.value(0.08),
            0.3,
        );
    }
    mb.rbox(
        at + Vec3::Y * 2.02,
        Vec3::new(1.5, 0.07, 0.1),
        0.03,
        Quat::IDENTITY,
        pal.timber_dark,
        0.1,
    );
    for side in [-1.0f32, 1.0] {
        mb.face(
            [
                at + Vec3::new(-1.55, 2.06, side * 0.44),
                at + Vec3::new(1.55, 2.06, side * 0.44),
                at + Vec3::new(1.55, 2.52, 0.0),
                at + Vec3::new(-1.55, 2.52, 0.0),
            ],
            Vec3::new(0.0, 1.0, side),
            roof * if side > 0.0 { 1.0 } else { 0.85 },
        );
        mb.face(
            [
                at + Vec3::new(-1.55, 2.05, side * 0.44),
                at + Vec3::new(1.55, 2.05, side * 0.44),
                at + Vec3::new(1.55, 2.51, 0.0),
                at + Vec3::new(-1.55, 2.51, 0.0),
            ],
            Vec3::new(0.0, -1.0, -side),
            pal.timber_dark,
        );
    }
    let glass = at + Vec3::Y * 1.62;
    mb.tube(
        at + Vec3::Y * 1.96,
        glass + Vec3::Y * 0.2,
        0.016,
        0.016,
        5,
        pal.bed,
        false,
    );
    lantern_cage(mb, glass, 0.1, 0.24, pal.brass);
    mb.lathe(
        glass + Vec3::Y * 0.12,
        &[(0.13, 0.0), (0.07, 0.08), (0.0, 0.12)],
        8,
        |_, _| pal.brass,
    );
    glass
}

/// The waystone: a round slab with a brass ring and a diamond let
/// into it, and four small stones standing about it.
fn waystone(mb: &mut Mb, rng: &mut Rng, pal: &Palette, at: Vec3) {
    let slate = pal.bed.lerp(pal.rock_dark, 0.5);
    mb.lathe(
        at,
        &[(1.2, -0.1), (1.2, 0.1), (1.08, 0.16), (0.0, 0.16)],
        20,
        |r, s| slate * (1.0 + ((r * 3 + s * 5) % 4) as f32 * 0.05),
    );
    mb.torus(at + Vec3::Y * 0.16, Vec3::Y, 0.84, 0.03, 24, pal.brass);
    mb.lathe(
        at,
        &[(0.44, 0.16), (0.36, 0.2), (0.0, 0.2)],
        4,
        |_, _| pal.pewter,
    );
    for i in 0..4 {
        let a = FRAC_PI_4 + i as f32 * TAU / 4.0;
        let half = Vec3::new(0.13, rng.range(0.24, 0.36), 0.1);
        mb.rbox(
            at + Vec3::new(a.sin(), 0.0, a.cos()) * 1.6
                + Vec3::Y * (half.y - 0.03),
            half,
            0.05,
            Quat::from_rotation_y(a) * rng.wobble(7.0),
            pal.rock * rng.value(0.08),
            0.3,
        );
    }
}

/// The letterbox, in the chief's colour, with its flag up and a
/// letter in its mouth.
fn letterbox(mb: &mut Mb, pal: &Palette, at: Vec3, paint: Vec3) {
    mb.rbox(
        at + Vec3::Y * 0.5,
        Vec3::new(0.05, 0.5, 0.05),
        0.02,
        Quat::IDENTITY,
        pal.timber,
        0.3,
    );
    mb.rbox(
        at + Vec3::Y * 1.1,
        Vec3::new(0.22, 0.12, 0.15),
        0.02,
        Quat::IDENTITY,
        paint,
        0.15,
    );
    mb.tube(
        at + Vec3::new(-0.22, 1.2, 0.0),
        at + Vec3::new(0.22, 1.2, 0.0),
        0.15,
        0.15,
        10,
        paint,
        true,
    );
    mb.rbox(
        at + Vec3::new(0.1, 1.33, 0.17),
        Vec3::new(0.05, 0.11, 0.012),
        0.005,
        Quat::IDENTITY,
        pal.wax,
        0.0,
    );
    mb.rbox(
        at + Vec3::new(-0.24, 1.12, 0.0),
        Vec3::new(0.05, 0.07, 0.1),
        0.004,
        Quat::from_rotation_z(0.25),
        pal.parchment,
        0.0,
    );
}

/// The flower bed, edged with stones, and the watering can by it.
fn bed(mb: &mut Mb, rng: &mut Rng, pal: &Palette, at: Vec3) {
    mb.lathe(
        at,
        &[(1.2, -0.1), (1.2, 0.06), (1.05, 0.1), (0.0, 0.1)],
        14,
        |r, s| pal.dirt_shade * (0.8 + ((r + s) % 3) as f32 * 0.05),
    );
    for i in 0..13 {
        let a = i as f32 / 13.0 * TAU + rng.sym(0.08);
        mb.rbox(
            at + Vec3::new(a.sin(), 0.0, a.cos()) * 1.22
                + Vec3::Y * 0.08,
            Vec3::new(0.16, 0.11, 0.11),
            0.04,
            Quat::from_rotation_y(a) * rng.wobble(5.0),
            pal.rock_warm * rng.value(0.08),
            0.2,
        );
    }
    for i in 0..11 {
        let a = i as f32 * 2.4 + rng.sym(0.2);
        let out = (i as f32 / 11.0).sqrt() * 0.92;
        let foot = at
            + Vec3::new(a.sin(), 0.0, a.cos()) * out
            + Vec3::Y * 0.1;
        let tall = rng.range(0.26, 0.5);
        let head =
            foot + Vec3::new(rng.sym(0.05), tall, rng.sym(0.05));
        mb.tube(
            foot,
            head,
            0.018,
            0.014,
            5,
            pal.foliage_shade,
            false,
        );
        let leaf = pal.foliage * rng.value(0.1);
        mb.blob(
            foot + Vec3::Y * 0.07,
            Vec3::new(0.13, 0.07, 0.13),
            4,
            0.2,
            rng.f() * 9.0,
            |_| leaf,
        );
        let petal = pal.flowers[i % pal.flowers.len()];
        mb.blob(
            head,
            Vec3::new(0.1, 0.075, 0.1),
            4,
            0.15,
            rng.f() * 9.0,
            |d| {
                petal.lerp(
                    pal.ochre,
                    if d.y > 0.7 { 0.8 } else { 0.0 },
                )
            },
        );
    }
    let can = at + Vec3::new(1.5, 0.0, -0.35);
    mb.lathe(
        can,
        &[(0.12, 0.0), (0.14, 0.2), (0.11, 0.22), (0.0, 0.22)],
        10,
        |_, _| pal.steel_bright,
    );
    mb.tube(
        can + Vec3::new(0.1, 0.08, 0.0),
        can + Vec3::new(0.33, 0.24, 0.0),
        0.03,
        0.02,
        6,
        pal.steel,
        true,
    );
    mb.torus(
        can + Vec3::new(-0.14, 0.13, 0.0),
        Vec3::Z,
        0.08,
        0.015,
        10,
        pal.steel,
    );
}

/// A bench with a back, facing `toward`.
fn bench(mb: &mut Mb, pal: &Palette, at: Vec3, toward: Vec2) {
    let yaw = Quat::from_rotation_y(toward.x.atan2(toward.y));
    mb.rbox(
        at + Vec3::Y * 0.42,
        Vec3::new(0.7, 0.035, 0.2),
        0.02,
        yaw,
        pal.log,
        0.2,
    );
    mb.rbox(
        at + yaw * Vec3::new(0.0, 0.72, -0.19),
        Vec3::new(0.7, 0.12, 0.03),
        0.02,
        yaw * Quat::from_rotation_x(-0.15),
        pal.log,
        0.1,
    );
    for side in [-1.0f32, 1.0] {
        mb.rbox(
            at + yaw * Vec3::new(side * 0.55, 0.2, 0.0),
            Vec3::new(0.04, 0.2, 0.16),
            0.02,
            yaw,
            pal.timber_dark,
            0.3,
        );
        mb.rbox(
            at + yaw * Vec3::new(side * 0.55, 0.6, -0.19),
            Vec3::new(0.035, 0.25, 0.03),
            0.015,
            yaw * Quat::from_rotation_x(-0.15),
            pal.timber_dark,
            0.1,
        );
    }
}

/// The meshes of the place of a chief who looks like `look`.
/// `ground` is the height of the land over the place's own origin.
pub fn build(
    pal: &Palette,
    set: &ChiefSettings,
    look: Look,
    ground: impl Fn(Vec2) -> f32,
) -> Built {
    let plan = Plan::of(set);
    let mut rng = Rng::seeded("the hearth of a chief");
    let (mut mb, mut panes) = (Mb::new(), Mb::new());
    let roof = look.cloak.colour(pal);
    let on = |at: Vec2| up(at, ground(at));

    let orb = house(&mut mb, &mut panes, pal, on(plan.house), roof);
    wall(&mut mb, &mut rng, pal, plan.wall, &ground);
    let gate_at = Vec2::new(0.0, plan.wall);
    let lantern = gate(&mut mb, &mut rng, pal, on(gate_at), roof);
    let stone = on(plan.waystone);
    waystone(&mut mb, &mut rng, pal, stone);
    letterbox(&mut mb, pal, on(plan.letterbox), roof);
    bed(&mut mb, &mut rng, pal, on(plan.bed));
    bench(&mut mb, pal, on(plan.bench), plan.waystone - plan.bench);

    // Stepping stones from the gate to the waystone and on to the
    // door.
    let door = plan.house.y + 2.9;
    let legs = [
        (plan.wall - 0.5, plan.waystone.y + 1.6),
        (plan.waystone.y - 1.6, door),
    ];
    for (from, to) in legs {
        let count = ((from - to) / 0.82).round().max(1.0) as usize;
        for k in 0..=count {
            let z = from + (to - from) * k as f32 / count as f32;
            let at = Vec2::new(rng.sym(0.14), z);
            let r = rng.range(0.28, 0.36);
            let tint = rng.value(0.07);
            mb.lathe(
                on(at),
                &[
                    (r, -0.05),
                    (r, 0.04),
                    (r * 0.82, 0.06),
                    (0.0, 0.06),
                ],
                8,
                |_, _| pal.limestone * tint,
            );
        }
    }

    for (a, out, r) in
        [(2.2f32, 0.86, 0.55), (4.3, 0.84, 0.48), (5.25, 0.8, 0.4)]
    {
        let at = Vec2::new(a.sin(), a.cos()) * out * plan.wall;
        let (light, dark) = (pal.foliage, pal.foliage_shade);
        mb.blob(
            on(at) + Vec3::Y * r * 0.55,
            Vec3::new(r, r * 0.75, r),
            5,
            0.22,
            a * 7.0,
            |d| dark.lerp(light, (d.y * 0.5 + 0.5).clamp(0.0, 1.0)),
        );
    }

    let r = plan.wall;
    let trees = [
        (Vec2::new(0.68, -0.22), 0, 1.45),
        (Vec2::new(-0.42, -0.66), 5, 1.25),
        (Vec2::new(-0.72, -0.2), 4, 1.0),
    ]
    .map(|(at, kind, scale)| Planted {
        at: on(at * r),
        kind,
        scale,
    });
    Built {
        body: mb,
        panes,
        lights: Lights {
            orb,
            lantern,
            ring: (stone + Vec3::Y * 0.19, 0.84),
        },
        trees,
    }
}
