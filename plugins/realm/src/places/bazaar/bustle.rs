//! What moves about the bazaar besides its people: the tinker's cogs
//! and grindstone, the costumier's turntable, the sparks off the
//! stone, the shimmer over the scrolls, the steam off the tea and the
//! embers of a sale, each kind of mote one set of copies.

use std::f32::consts::{PI, TAU};

use glam::{Quat, Vec3};
use hamlet_engine_sdk::scene::{
    Entity, Instances, Material, Mesh, Shape, copies,
};

use crate::build::frame::Frame;
use crate::build::stock::glow;
use crate::build::upload::Upload;
use crate::places::bazaar::build::{
    FLOOR, SHIMMER, SPARKS, TEA, TEAPOT, at_stall,
};
use crate::places::bazaar::keepers::working;
use crate::places::bazaar::{BazaarSettings, Stall};
use crate::world::palette::Palette;

/// Where the embers of a sale rise from, from its stall's middle:
/// over its counter.
const SALE: Vec3 = Vec3::new(0.0, FLOOR + 1.0, 0.45);

/// A number in 0..1 that follows from `n` and looks random.
fn chance(n: f32) -> f32 {
    (n * 12.9898 + 4.1414)
        .sin()
        .mul_add(43_758.547, 0.0)
        .fract()
        .abs()
}

/// A part that turns on its own, the way it lies at rest, its turns
/// a second and the turn it starts at, in radians.
pub struct Turning {
    pub entity: Entity,
    pub at: Vec3,
    pub lies: Quat,
    pub rate: f32,
    pub phase: f32,
}

impl Turning {
    /// Turns it to clock time `now`.
    pub fn turn(&self, now: f32) {
        let turn = self.lies
            * Quat::from_rotation_z(
                self.phase + TAU * (self.rate * now).fract(),
            );
        self.entity.set_transform(
            Frame::at(self.at).with_turn(turn).scene(),
        );
    }
}

/// A kind of mote.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Spark,
    Shimmer,
    Steam,
    Ember,
}

impl Kind {
    const ALL: [Kind; 4] =
        [Kind::Spark, Kind::Shimmer, Kind::Steam, Kind::Ember];

    /// How many motes it is made of, the seconds each lasts, its
    /// size and its light.
    fn of(self, set: &BazaarSettings) -> (usize, f32, f32, f32) {
        match self {
            Kind::Spark => set.sparks,
            Kind::Shimmer => set.shimmer,
            Kind::Steam => set.steam,
            Kind::Ember => set.embers,
        }
    }

    fn colour(self, pal: &Palette) -> Vec3 {
        match self {
            Kind::Spark | Kind::Ember => pal.ember,
            Kind::Shimmer => {
                pal.bazaar.stalls[Stall::Skills as usize].glow
            }
            Kind::Steam => pal.whitewash,
        }
    }
}

/// The motes of each kind, as copies of one ball.
pub struct Motes {
    sets: Vec<(Kind, Instances)>,
    /// Whether each set was last given any mote to show.
    shown: [bool; 4],
    /// The ball and the motes' looks, kept so that they stay.
    _ball: Mesh,
    _looks: Vec<Material>,
}

impl Motes {
    /// The motes under `parent`, none shown yet.
    pub fn make(
        parent: &Entity,
        set: &BazaarSettings,
        pal: &Palette,
    ) -> Result<Self, String> {
        let ball = Mesh::shape(Shape::Sphere(1.0))?;
        let mut looks = Vec::new();
        let mut sets = Vec::new();
        for kind in Kind::ALL {
            let (_, _, _, light) = kind.of(set);
            let look = Material::make(glow(kind.colour(pal), light))?;
            let copies =
                Instances::make(Some(parent), &ball, &look, 0)?;
            looks.push(look);
            sets.push((kind, copies));
        }
        Ok(Self {
            sets,
            shown: [true; 4],
            _ball: ball,
            _looks: looks,
        })
    }

    /// Moves the motes to clock time `now`. The sparks fly while the
    /// tinker works its stone and is not `called`; the embers rise
    /// over the stall of a sale and when, while it is cheered.
    pub fn stir(
        &mut self,
        set: &BazaarSettings,
        now: f32,
        called: bool,
        sale: Option<(Stall, f32)>,
        upload: &mut Upload,
    ) {
        for (i, (kind, copies_of)) in self.sets.iter().enumerate() {
            let (count, ..) = kind.of(set);
            let placed = (0..count)
                .filter_map(|k| {
                    let (at, size) =
                        mote(set, (*kind, k), now, called, sale)?;
                    Some(
                        Frame::at(at)
                            .with_size(Vec3::splat(size))
                            .scene(),
                    )
                })
                .collect::<Vec<_>>();
            let any = !placed.is_empty();
            if !any && !self.shown[i] {
                continue;
            }
            if upload.copies(copies_of, &copies(&placed)) {
                self.shown[i] = any;
            }
        }
    }
}

/// Where mote `k` of `kind` is at `now`, from the bazaar's middle,
/// and its size. None while it is not shown.
fn mote(
    set: &BazaarSettings,
    (kind, k): (Kind, usize),
    now: f32,
    called: bool,
    sale: Option<(Stall, f32)>,
) -> Option<(Vec3, f32)> {
    let (count, life, size, _) = kind.of(set);
    let u = (now / life + k as f32 / count as f32).fract();
    let (at, size) = match kind {
        Kind::Spark => {
            if called || !working(Stall::Plugins, now) {
                return None;
            }
            let cycle = now / life + k as f32 / count as f32;
            let seed = cycle.floor() * count as f32 + k as f32;
            // Off the stone forward and down, in a fan.
            let way = Vec3::new(
                -0.35 + 0.8 * (chance(seed) - 0.5),
                -0.1 + 0.7 * chance(seed + 0.3),
                1.0,
            )
            .normalize();
            let speed = 1.4 + 1.4 * chance(seed + 0.6);
            let s = u * life;
            let mut at =
                SPARKS + way * speed * s - Vec3::Y * 4.0 * s * s;
            at.y = at.y.max(FLOOR + 0.02);
            (at_stall(Stall::Plugins, at), size * (1.0 - u))
        }
        Kind::Shimmer => {
            let a = k as f32 / count as f32 * TAU + u * 2.4;
            let reach = 0.18 + 0.12 * (u * PI).sin();
            let at = SHIMMER
                + Vec3::new(
                    a.cos() * reach,
                    u * 1.1,
                    a.sin() * reach,
                );
            (at_stall(Stall::Skills, at), size * (u * PI).sin())
        }
        Kind::Steam => {
            let (at, turn) = TEA;
            let drift = Vec3::new(
                0.05 * (u * 5.0 + k as f32).sin(),
                u * 0.75,
                0.04 * (u * 4.0 + k as f32 * 1.3).cos(),
            );
            let at =
                at + Quat::from_rotation_y(turn) * (TEAPOT + drift);
            (at, size * (0.3 + 0.7 * u) * (1.0 - u).sqrt())
        }
        Kind::Ember => {
            let (stall, began) = sale?;
            // Each rises a little after the last, and once.
            let own = k as f32 + chance(began + k as f32);
            let wait = chance(own + 0.5) * life * 0.6;
            let u = (now - began - wait) / life;
            if !(0.0..1.0).contains(&u) {
                return None;
            }
            let a = own * 2.4 + u * 3.0;
            let reach = 0.25 + 0.5 * chance(own + 0.25);
            let at = SALE
                + Vec3::new(
                    a.cos() * reach,
                    u * (1.4 + chance(own) * 1.2),
                    a.sin() * reach * 0.5,
                );
            (
                at_stall(stall, at),
                size * (u * 6.0).min(1.0) * (1.0 - u),
            )
        }
    };
    (size > 0.001).then_some((at, size))
}
