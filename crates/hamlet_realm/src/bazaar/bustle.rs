//! What moves about the bazaar besides its people: the tinker's cogs
//! and grindstone, the sparks off the stone, the shimmer over the
//! scrolls, the steam off the tea and the embers after a sale.

use std::f32::consts::{PI, TAU};

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::window::RequestRedraw;
use hamlet_vfx::magic::{Ember, Wand, chance, glow, rise};

use super::build::{
    FLOOR, PAVILION, SHIMMER, SPARKS, TEA, TEAPOT, at_stall,
};
use super::keepers::working;
use super::pitch::Making;
use super::{Bazaar, BazaarSettings, Stall, Traded};
use crate::Clock;
use crate::pick::{Selected, Thing};

/// Where the embers rise from after a sale, from the bazaar's
/// middle.
const SALE: Vec3 = Vec3::new(0.0, FLOOR + 1.1, 2.2);

/// A part that turns on its own, the way it lies at rest and its
/// turns a second.
#[derive(Component, Clone, Copy, Debug)]
pub(super) struct Turning {
    pub lies: Quat,
    pub rate: f32,
}

/// A mote of light: what it is part of, and its number among them.
#[derive(Component, Clone, Copy, Debug)]
pub(super) struct Mote(Kind, usize);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Spark,
    Shimmer,
    Steam,
}

impl Kind {
    /// How many motes it is made of, the seconds each lasts, its
    /// size and its light.
    fn of(self, set: &BazaarSettings) -> (usize, f32, f32, f32) {
        match self {
            Kind::Spark => set.sparks,
            Kind::Shimmer => set.shimmer,
            Kind::Steam => set.steam,
        }
    }
}

/// Sets the motes about the bazaar, under `small`.
pub(super) fn scatter(
    commands: &mut Commands,
    making: &mut Making,
    set: &BazaarSettings,
    small: Entity,
) {
    let mote = making.meshes.add(Sphere::new(1.0).mesh().uv(8, 6));
    let pal = making.pal;
    let skill = pal.stalls[Stall::Skills as usize].glow;
    for (kind, colour) in [
        (Kind::Spark, pal.ember),
        (Kind::Shimmer, skill),
        (Kind::Steam, pal.whitewash),
    ] {
        let (count, _, _, light) = kind.of(set);
        let material = making.materials.add(glow(colour, light));
        for k in 0..count {
            commands.spawn((
                Mote(kind, k),
                Mesh3d(mote.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_scale(Vec3::ZERO),
                NotShadowCaster,
                ChildOf(small),
            ));
        }
    }
}

/// Turns each part that turns.
pub(super) fn spin(
    clock: Res<Clock>,
    mut parts: Query<(&Turning, &mut Transform)>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    for (part, mut at) in &mut parts {
        at.rotation = part.lies
            * Quat::from_rotation_z(
                TAU * (part.rate * clock.t).fract(),
            );
    }
    redraw.write(RequestRedraw);
}

/// Where mote `k` of `kind` is at `t`, from the bazaar's middle, and
/// its size. Nothing is drawn of one of size 0. The sparks fly while
/// the tinker works its stone, and not while `called` to a visitor.
fn mote(
    set: &BazaarSettings,
    (kind, k): (Kind, usize),
    t: f32,
    called: bool,
) -> (Vec3, f32) {
    let (count, life, size, _) = kind.of(set);
    let u = (t / life + k as f32 / count as f32).fract();
    match kind {
        Kind::Spark => {
            if called || !working(Stall::Plugins, t) {
                return (Vec3::ZERO, 0.0);
            }
            let cycle = t / life + k as f32 / count as f32;
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
    }
}

/// Moves the motes.
pub(super) fn stir(
    clock: Res<Clock>,
    set: Res<BazaarSettings>,
    selected: Res<Selected>,
    mut motes: Query<(&Mote, &mut Transform, &mut Visibility)>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    let called = selected.0 == Some(Thing::Stall(Stall::Plugins));
    for (&Mote(kind, k), mut at, mut shown) in &mut motes {
        let (spot, size) = mote(&set, (kind, k), clock.t, called);
        shown.set_if_neq(match size > 0.001 {
            true => Visibility::Inherited,
            false => Visibility::Hidden,
        });
        let to = Transform::from_translation(spot)
            .with_scale(Vec3::splat(size));
        if *at != to {
            *at = to;
        }
    }
    redraw.write(RequestRedraw);
}

/// The chief bought something: embers rise from the pavilion's
/// counter.
pub(super) fn celebrate(
    _: On<Traded>,
    mut commands: Commands,
    clock: Res<Clock>,
    set: Res<BazaarSettings>,
    wand: Option<Res<Wand>>,
    bazaars: Query<&GlobalTransform, With<Bazaar>>,
) {
    // A still shows the moment after, with no magic in the air.
    let Some(wand) = wand.filter(|_| !clock.frozen) else {
        return;
    };
    for place in &bazaars {
        for i in 0..set.embers {
            let own = i as f32 + chance(clock.t + i as f32);
            let a = own * 2.4;
            let from = PAVILION
                + SALE
                + Vec3::new(a.cos() * 1.2, 0.0, a.sin() * 0.3);
            rise(
                &mut commands,
                &wand,
                Ember {
                    began: clock.t,
                    from: place.transform_point(from),
                    own,
                },
            );
        }
    }
}
