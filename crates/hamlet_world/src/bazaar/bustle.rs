//! What moves about the bazaar besides its people: the tinker's cogs
//! and grindstone, the sparks off the stone, the shimmer over the
//! skill-scrolls, the steam off the tea, and embers that rise from
//! the counter after a sale. Each is a pure function of the clock.

use std::f32::consts::{PI, TAU};

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::window::RequestRedraw;
use hamlet_vfx::magic::{Ember, Wand, chance, glow, rise};

use super::build::{
    FLOOR, PAVILION, SHIMMER, SPARKS, TEA, TEAPOT, at_stall,
};
use super::keepers::working;
use super::stalls::Stall;
use super::{Bazaar, Traded};
use crate::Clock;
use crate::palette::Palette;
use crate::pick::{Selected, Thing};

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

/// How many motes each is made of, the seconds each mote lasts, its
/// size and its light.
const SPARK: (usize, f32, f32, f32) = (16, 0.55, 0.03, 9.0);
const SHIMMERS: (usize, f32, f32, f32) = (12, 2.6, 0.035, 5.0);
const STEAM: (usize, f32, f32, f32) = (6, 3.2, 0.11, 0.35);

/// The embers that rise from the counter after a sale, and where
/// they rise from, from the bazaar's middle.
const EMBERS: usize = 18;
const SALE: Vec3 = Vec3::new(0.0, FLOOR + 1.1, 2.2);

/// Sets the motes about the bazaar, under it.
pub(super) fn scatter(
    commands: &mut Commands,
    (meshes, materials, pal): (
        &mut Assets<Mesh>,
        &mut Assets<StandardMaterial>,
        &Palette,
    ),
    bazaar: Entity,
) {
    let mote = meshes.add(Sphere::new(1.0).mesh().uv(8, 6));
    let skill = pal.stalls[Stall::Skills as usize].glow;
    for (kind, (count, _, _, light), colour) in [
        (Kind::Spark, SPARK, pal.ember),
        (Kind::Shimmer, SHIMMERS, skill),
        (Kind::Steam, STEAM, pal.whitewash),
    ] {
        let material = materials.add(glow(colour, light));
        for k in 0..count {
            commands.spawn((
                Mote(kind, k),
                Mesh3d(mote.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_scale(Vec3::ZERO),
                NotShadowCaster,
                ChildOf(bazaar),
            ));
        }
    }
}

/// Turns each part that turns, while the bazaar is seen.
pub(super) fn spin(
    clock: Res<Clock>,
    bazaars: Query<(&ViewVisibility, &super::Lod), With<Bazaar>>,
    mut parts: Query<(&Turning, &mut Transform)>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    if parts.is_empty()
        || !bazaars.iter().any(|(seen, lod)| seen.get() && lod.close)
    {
        return;
    }
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
fn mote(kind: Kind, k: usize, t: f32, called: bool) -> (Vec3, f32) {
    match kind {
        Kind::Spark => {
            let (count, life, size, _) = SPARK;
            if called || !working(Stall::Plugins, t) {
                return (Vec3::ZERO, 0.0);
            }
            let cycle = t / life + k as f32 / count as f32;
            let (n, u) = (cycle.floor(), cycle.fract());
            let seed = n * count as f32 + k as f32;
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
            let (count, life, size, _) = SHIMMERS;
            let u = (t / life + k as f32 / count as f32).fract();
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
            let (count, life, size, _) = STEAM;
            let u = (t / life + k as f32 / count as f32).fract();
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

/// Moves the motes, while the bazaar is seen.
pub(super) fn stir(
    clock: Res<Clock>,
    selected: Res<Selected>,
    bazaars: Query<(&ViewVisibility, &super::Lod), With<Bazaar>>,
    mut motes: Query<(&Mote, &mut Transform, &mut Visibility)>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    if motes.is_empty()
        || !bazaars.iter().any(|(seen, lod)| seen.get() && lod.close)
    {
        return;
    }
    let called = selected.0 == Some(Thing::Stall(Stall::Plugins));
    for (&Mote(kind, k), mut at, mut shown) in &mut motes {
        let (spot, size) = mote(kind, k, clock.t, called);
        let want = match size > 0.001 {
            true => Visibility::Inherited,
            false => Visibility::Hidden,
        };
        shown.set_if_neq(want);
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
    wand: Option<Res<Wand>>,
    bazaars: Query<&GlobalTransform, With<Bazaar>>,
) {
    // A still shows the moment after, with no magic in the air.
    let Some(wand) = wand.filter(|_| !clock.frozen) else {
        return;
    };
    for place in &bazaars {
        for i in 0..EMBERS {
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
