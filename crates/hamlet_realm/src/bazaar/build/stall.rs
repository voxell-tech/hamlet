//! A stall of the bazaar: what every stall is built of, and each
//! stall itself in a file of its own.

mod gear;
mod land;
mod minds;
mod scrolls;
mod tinker;
mod workshop;

use bevy::prelude::*;

use self::gear::{Gear, cog};
pub(in crate::bazaar) use self::scrolls::SHIMMER;
pub(in crate::bazaar) use self::tinker::SPARKS;
use super::{Cog, FLOOR, Shapes, lantern};
use crate::bazaar::Stall;
use crate::meshes::Meshing;
use crate::palette::{Palette, StallStyle};

/// A stall's posts stand this far either side of its middle, and
/// this far before and behind it.
const WIDE: f32 = 1.45;
const DEEP: f32 = 0.8;

/// The height of a stall's front posts and of its back ones.
const LOW: f32 = 2.4;
const HIGH: f32 = 2.95;

/// Where a stall's keeper stands, from the stall's middle: before
/// its counter, at the end of it.
pub(in crate::bazaar) const KEEPER: Vec3 =
    Vec3::new(1.2, FLOOR, 1.05);

/// Where the work a stall's keeper keeps to stands, beside its
/// keeper and out from the stall's side.
const WORK: Vec3 = Vec3::new(WIDE + 0.55, FLOOR, 1.0);

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

/// A stall being built, facing +Z on its own spot: its cloth and
/// timber, the glass of its lantern, what glows among its wares and
/// its colours.
pub(super) struct Stand<'a> {
    pub mb: &'a mut Meshing,
    pub glass: &'a mut Meshing,
    pub gleam: &'a mut Meshing,
    pub pal: &'a Palette,
    pub style: StallStyle,
}

/// Builds `stall`.
pub(super) fn stall(
    mb: &mut Meshing,
    (glass, gleam): (&mut Meshing, &mut Meshing),
    pal: &Palette,
    stall: Stall,
) -> Dressed {
    let mut s = Stand {
        mb,
        glass,
        gleam,
        pal,
        style: pal.stalls[stall as usize],
    };
    match stall {
        Stall::Models => minds::build(&mut s),
        Stall::Community => workshop::build(&mut s),
        Stall::Plugins => tinker::build(&mut s),
        Stall::Skills => scrolls::build(&mut s),
        Stall::Hamlets => land::build(&mut s),
    }
}

/// The way a stall's keeper faces to work: toward its work, turned
/// a little out to the promenade so that its face shows.
pub(in crate::bazaar) fn at_work() -> f32 {
    let way = WORK - KEEPER;
    way.x.atan2(way.z) - 0.45
}

/// The `n` strips across from `-edge` to `edge`: the number of each
/// and its two sides.
fn strips(
    edge: f32,
    n: usize,
) -> impl Iterator<Item = (usize, f32, f32)> {
    let side =
        move |i: usize| -edge + 2.0 * edge * i as f32 / n as f32;
    (0..n).map(move |i| (i, side(i), side(i + 1)))
}

/// The two ends of the front of an awning.
fn awning_ends() -> [Vec3; 2] {
    [-1.0f32, 1.0].map(|side| {
        Vec3::new(side * (WIDE + 0.25), LOW - 0.12, DEEP + 0.5)
    })
}

/// A cog of `gear` and `colour` that turns `rate` times a second,
/// from `phase`, about an axle at `at` that lies along Z.
fn turning(
    at: Vec3,
    gear: Gear,
    colour: Vec3,
    (rate, phase): (f32, f32),
) -> Cog {
    let mut mb = Meshing::new();
    cog(&mut mb, gear, colour);
    Cog {
        mesh: mb.build(),
        at,
        lies: Quat::IDENTITY,
        rate,
        phase,
    }
}

impl Stand<'_> {
    /// Four posts of `wood` at the corners, the front ones `front`
    /// tall and the back ones `back`.
    fn posts(&mut self, wood: Vec3, (front, back): (f32, f32)) {
        for (x, z, tall) in [
            (-WIDE, DEEP, front),
            (WIDE, DEEP, front),
            (-WIDE, -DEEP, back),
            (WIDE, -DEEP, back),
        ] {
            self.mb.tube(
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
    fn rail(&mut self, y: f32) {
        self.mb.rod(
            Vec3::new(-WIDE, y, DEEP),
            Vec3::new(WIDE, y, DEEP),
            0.025,
            5,
            self.pal.timber_dark,
            true,
        );
    }

    /// A striped awning from the top of the back posts out over the
    /// front, ending in a flap to each stripe tipped with brass.
    fn awning(&mut self, stripes: [Vec3; 2]) {
        let (back, front) =
            ((-DEEP - 0.15, HIGH + 0.05), (DEEP + 0.5, LOW - 0.1));
        for (i, x0, x1) in strips(WIDE + 0.25, 8) {
            let colour = stripes[i % 2];
            self.mb.cloth(
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
            self.mb.flap(
                (
                    Vec3::new(x0, front.1, front.0),
                    Vec3::new(x1, front.1, front.0),
                ),
                tip,
                Vec3::Z,
                colour,
            );
            self.mb.ball(tip, Vec3::splat(0.025), 2, self.pal.brass);
        }
    }

    /// A cloth across the back, from the floor to `tall`.
    fn backdrop(&mut self, colour: Vec3, tall: f32) {
        let z = -DEEP - 0.03;
        self.mb.cloth(
            [
                Vec3::new(-WIDE, FLOOR, z),
                Vec3::new(WIDE, FLOOR, z),
                Vec3::new(WIDE, tall, z),
                Vec3::new(-WIDE, tall, z),
            ],
            Vec3::Z,
            colour,
        );
    }

    /// A counter across the front of `wood`, its front hung with a
    /// band of its stall's cloth. Returns the middle of its top.
    fn counter(&mut self, wood: Vec3) -> Vec3 {
        let (mb, pal, style) = (&mut *self.mb, self.pal, self.style);
        let at = Vec3::new(0.0, FLOOR, 0.45);
        mb.slab(
            at + Vec3::Y * 0.42,
            Vec3::new(0.95, 0.42, 0.28),
            wood,
        );
        mb.slab(
            at + Vec3::Y * 0.87,
            Vec3::new(1.05, 0.03, 0.34),
            pal.timber_dark,
        );
        for (z, half_y, colour) in
            [(0.285, 0.13, style.cloth), (0.292, 0.02, style.trim)]
        {
            mb.plate(
                at + Vec3::new(0.0, 0.6, z),
                Vec3::new(0.93, half_y, 0.006),
                Quat::IDENTITY,
                colour,
            );
        }
        at + Vec3::Y * 0.9
    }

    /// A sign board of the stall's cloth edged in its trim, facing
    /// +Z with its middle at `at`, on two brass struts down to
    /// `foot`. Returns the middle of its face and its top.
    fn sign(&mut self, at: Vec3, foot: f32) -> (Vec3, Vec3) {
        let half = Vec2::new(0.62, 0.22);
        self.mb.slab(
            at,
            (half + 0.05).extend(0.035),
            self.style.trim,
        );
        self.mb.slab(
            at + Vec3::Z * 0.012,
            half.extend(0.03),
            self.style.cloth,
        );
        for side in [-1.0f32, 1.0] {
            let x = at.x + side * half.x * 0.7;
            self.mb.rod(
                Vec3::new(x, at.y - half.y, at.z - 0.02),
                Vec3::new(x, foot, at.z - 0.02),
                0.015,
                4,
                self.pal.brass,
                false,
            );
        }
        (at + Vec3::Z * 0.045, at + Vec3::Y * (half.y + 0.05))
    }

    /// Hangs its lantern from `lamp` and puts a mast over the post
    /// whose top is at `post`, and gives back what was built.
    fn finish(
        &mut self,
        (lamp, post): (Vec3, Vec3),
        crest: Vec3,
        ties: [Vec3; 2],
        cogs: Vec<Cog>,
    ) -> Dressed {
        let pal = self.pal;
        let lamp = lantern(self.mb, self.glass, pal, lamp);
        let mb = &mut *self.mb;
        mb.tube(
            post,
            post + Vec3::Y * 0.7,
            0.03,
            0.02,
            5,
            pal.timber_dark,
            true,
        );
        mb.ball(
            post + Vec3::Y * 0.72,
            Vec3::splat(0.04),
            2,
            pal.brass,
        );
        Dressed {
            lamp,
            mast: post + Vec3::Y * 0.67,
            crest,
            ties,
            cogs,
        }
    }
}
