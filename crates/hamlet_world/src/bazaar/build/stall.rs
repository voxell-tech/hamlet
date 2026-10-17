//! A stall of the bazaar: what every stall is built of, and each
//! stall itself, in a file of its own.

mod land;
mod minds;
mod scrolls;
mod tinker;
mod workshop;

use bevy::math::Affine3A;
use bevy::prelude::*;

pub(in crate::bazaar) use self::scrolls::SHIMMER;
pub(in crate::bazaar) use self::tinker::SPARKS;
use super::{Cog, FLOOR, cloth, slab};
use crate::bazaar::stalls::Stall;
use crate::meshes::Mb;
use crate::palette::{Palette, StallStyle};

/// A stall's posts stand this far either side of its middle, and
/// this far before and behind it.
const WIDE: f32 = 1.45;
const DEEP: f32 = 0.8;

/// The height of a stall's front posts and of its back ones.
const LOW: f32 = 2.4;
const HIGH: f32 = 2.95;

/// What a stall's building gives back, from its own middle: the
/// spot of its lantern, where its flag hangs, the top of its sign,
/// the two ends of its front for strings of lights, and its parts
/// that turn.
pub(super) struct Dressed {
    pub lamp: Vec3,
    pub mast: Vec3,
    pub crest: Vec3,
    pub ties: [Vec3; 2],
    pub cogs: Vec<Cog>,
}

/// One stall, built facing +Z on its own spot, with what glows
/// among its wares in `gleam`.
pub(super) fn stall(
    mb: &mut Mb,
    (glass, gleam): (&mut Mb, &mut Mb),
    pal: &Palette,
    stall: Stall,
) -> Dressed {
    let style = pal.stalls[stall as usize];
    match stall {
        Stall::Models => minds::build(mb, (glass, gleam), pal, style),
        Stall::Community => {
            workshop::build(mb, (glass, gleam), pal, style)
        }
        Stall::Plugins => {
            tinker::build(mb, (glass, gleam), pal, style)
        }
        Stall::Skills => {
            scrolls::build(mb, (glass, gleam), pal, style)
        }
        Stall::Hamlets => land::build(mb, (glass, gleam), pal, style),
    }
}

/// Where a stall's keeper stands, from the stall's middle: before
/// its counter, at the end of it.
pub(in crate::bazaar) const KEEPER: Vec3 =
    Vec3::new(1.2, FLOOR, 1.05);

/// Where the work a stall's keeper keeps to stands, beside its
/// keeper and out from the stall's side.
const WORK: Vec3 = Vec3::new(WIDE + 0.55, FLOOR, 1.0);

/// The way a stall's keeper faces to work: toward its work, turned
/// a little out to the promenade so that its face shows.
pub(in crate::bazaar) fn at_work() -> f32 {
    let way = WORK - KEEPER;
    way.x.atan2(way.z) - 0.45
}

/// Four posts of `wood` at the corners, the front ones `front` tall
/// and the back ones `back`.
fn posts(mb: &mut Mb, wood: Vec3, (front, back): (f32, f32)) {
    for (x, z, tall) in [
        (-WIDE, DEEP, front),
        (WIDE, DEEP, front),
        (-WIDE, -DEEP, back),
        (WIDE, -DEEP, back),
    ] {
        mb.tube(
            Vec3::new(x, FLOOR, z),
            Vec3::new(x, tall + 0.05, z),
            0.065,
            0.055,
            6,
            wood,
            true,
        );
    }
}

/// A rail between the front posts at `y`.
fn rail(mb: &mut Mb, pal: &Palette, y: f32) {
    mb.tube(
        Vec3::new(-WIDE, y, DEEP),
        Vec3::new(WIDE, y, DEEP),
        0.025,
        0.025,
        5,
        pal.timber_dark,
        true,
    );
}

/// A striped awning from the top of the back posts out over the
/// front, ending in a flap to each stripe tipped with brass.
fn awning(mb: &mut Mb, pal: &Palette, stripes: [Vec3; 2]) {
    let (back, front) =
        ((-DEEP - 0.15, HIGH + 0.05), (DEEP + 0.5, LOW - 0.1));
    let (edge, strips) = (WIDE + 0.25, 8);
    for i in 0..strips {
        let (x0, x1) = (
            -edge + 2.0 * edge * i as f32 / strips as f32,
            -edge + 2.0 * edge * (i + 1) as f32 / strips as f32,
        );
        let colour = stripes[i % 2];
        cloth(
            mb,
            [
                Vec3::new(x0, back.1, back.0),
                Vec3::new(x1, back.1, back.0),
                Vec3::new(x1, front.1, front.0),
                Vec3::new(x0, front.1, front.0),
            ],
            Vec3::new(0.0, 1.0, 0.45),
            colour,
        );
        let tip = Vec3::new(
            (x0 + x1) * 0.5,
            front.1 - 0.24,
            front.0 + 0.01,
        );
        cloth(
            mb,
            [
                Vec3::new(x0, front.1, front.0),
                Vec3::new(x1, front.1, front.0),
                tip,
                tip,
            ],
            Vec3::Z,
            colour,
        );
        mb.blob(tip, Vec3::splat(0.025), 2, 0.0, 0.0, |_| pal.brass);
    }
}

/// The two ends of the front of an awning.
fn awning_ends() -> [Vec3; 2] {
    [-1.0f32, 1.0].map(|side| {
        Vec3::new(side * (WIDE + 0.25), LOW - 0.12, DEEP + 0.5)
    })
}

/// A cloth across the back, from the floor to `tall`.
fn backdrop(mb: &mut Mb, colour: Vec3, tall: f32) {
    cloth(
        mb,
        [
            Vec3::new(-WIDE, FLOOR, -DEEP - 0.03),
            Vec3::new(WIDE, FLOOR, -DEEP - 0.03),
            Vec3::new(WIDE, tall, -DEEP - 0.03),
            Vec3::new(-WIDE, tall, -DEEP - 0.03),
        ],
        Vec3::Z,
        colour,
    );
}

/// A counter across the front of `wood`, its front hung with a band
/// of its stall's cloth. Returns the middle of its top.
fn counter(
    mb: &mut Mb,
    pal: &Palette,
    style: StallStyle,
    wood: Vec3,
) -> Vec3 {
    let at = Vec3::new(0.0, FLOOR, 0.45);
    slab(mb, at + Vec3::Y * 0.42, Vec3::new(0.95, 0.42, 0.28), wood);
    slab(
        mb,
        at + Vec3::Y * 0.87,
        Vec3::new(1.05, 0.03, 0.34),
        pal.timber_dark,
    );
    mb.rbox(
        at + Vec3::new(0.0, 0.6, 0.285),
        Vec3::new(0.93, 0.13, 0.006),
        0.0,
        Quat::IDENTITY,
        style.cloth,
        0.0,
    );
    mb.rbox(
        at + Vec3::new(0.0, 0.6, 0.292),
        Vec3::new(0.93, 0.02, 0.006),
        0.0,
        Quat::IDENTITY,
        style.trim,
        0.0,
    );
    at + Vec3::Y * 0.9
}

/// A mast over a post whose top is at `top`. Returns where its flag
/// hangs.
fn mast(mb: &mut Mb, pal: &Palette, top: Vec3) -> Vec3 {
    mb.tube(
        top,
        top + Vec3::Y * 0.7,
        0.03,
        0.02,
        5,
        pal.timber_dark,
        true,
    );
    mb.blob(
        top + Vec3::Y * 0.72,
        Vec3::splat(0.04),
        2,
        0.0,
        0.0,
        |_| pal.brass,
    );
    top + Vec3::Y * 0.67
}

/// A sign board of the stall's cloth edged in its trim, facing +Z
/// with its middle at `at`, on two brass struts down to `foot`.
/// Returns the middle of its face and its top.
fn sign(
    mb: &mut Mb,
    pal: &Palette,
    style: StallStyle,
    at: Vec3,
    foot: f32,
) -> (Vec3, Vec3) {
    let half = Vec2::new(0.62, 0.22);
    slab(mb, at, (half + 0.05).extend(0.035), style.trim);
    slab(mb, at + Vec3::Z * 0.012, half.extend(0.03), style.cloth);
    for side in [-1.0f32, 1.0] {
        mb.tube(
            at + Vec3::new(side * half.x * 0.7, -half.y, -0.02),
            Vec3::new(at.x + side * half.x * 0.7, foot, at.z - 0.02),
            0.015,
            0.015,
            4,
            pal.brass,
            false,
        );
    }
    (at + Vec3::Z * 0.045, at + Vec3::Y * (half.y + 0.05))
}

/// A cog about an axle along Z: its rim with `teeth` teeth, its
/// spokes and its hub.
fn cog(mb: &mut Mb, radius: f32, teeth: usize, colour: Vec3) {
    let thick = 0.04 + radius * 0.06;
    mb.torus(
        Vec3::ZERO,
        Vec3::Z,
        radius * 0.82,
        radius * 0.12,
        20,
        colour,
    );
    for k in 0..teeth {
        let a = k as f32 / teeth as f32 * std::f32::consts::TAU;
        let out = Vec3::new(a.cos(), a.sin(), 0.0);
        mb.rbox(
            out * radius * 0.98,
            Vec3::new(radius * 0.12, radius * 0.09, thick),
            0.01,
            Quat::from_rotation_z(a),
            colour,
            0.0,
        );
    }
    for k in 0..4 {
        let a = k as f32 / 4.0 * std::f32::consts::TAU + 0.4;
        mb.tube(
            Vec3::ZERO,
            Vec3::new(a.cos(), a.sin(), 0.0) * radius * 0.78,
            radius * 0.06,
            radius * 0.05,
            5,
            colour * 0.85,
            false,
        );
    }
    mb.tube(
        Vec3::Z * -thick * 1.4,
        Vec3::Z * thick * 1.4,
        radius * 0.18,
        radius * 0.18,
        10,
        colour * 0.7,
        true,
    );
}

/// Builds `part` at `local` within what `mb` builds at already.
fn within(mb: &mut Mb, local: Affine3A, part: impl FnOnce(&mut Mb)) {
    let outer = mb.xf;
    mb.at(outer * local);
    part(mb);
    mb.at(outer);
}

/// A cog of `radius` and `teeth` of `colour` that turns `rate` times
/// a second about an axle at `at` that lies along `lies` * Z.
fn turning(
    (at, lies): (Vec3, Quat),
    (radius, teeth): (f32, usize),
    colour: Vec3,
    rate: f32,
) -> Cog {
    let mut mb = Mb::new();
    cog(&mut mb, radius, teeth, colour);
    Cog {
        mesh: mb.build(),
        at,
        lies,
        rate,
    }
}
