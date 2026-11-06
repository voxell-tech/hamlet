//! A boat that carries a traveler over the sea: a copy of the one
//! moored at the island it leaves, which stands in for it meanwhile.

use std::f32::consts::{PI, TAU};

use bevy::prelude::*;
use bevy::window::RequestRedraw;

use super::{Boat, Harbor, ROUTE_LIFT, rock, shapes};
use crate::Clock;
use crate::generator::land::{IslandId, Land};
use crate::keeper::{Motion, Trip};

/// Ground a ferry covers per second.
const SPEED: f32 = 6.0;

/// How high over the still water a ferry rides in the open sea,
/// clear of the waves.
const OPEN_LIFT: f32 = ROUTE_LIFT * 0.4;

/// How near a moored boat a ferry hides it.
const BERTH: f32 = 4.0;

/// Distance from its berth over which a ferry turns to lie as the
/// moored boat does.
const TURNS_OVER: f32 = 10.0;

/// A boat under way. It follows its [`Trip`].
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct Ferry {
    /// The island whose moored boat it is a copy of.
    home: IslandId,
}

/// The sail from the berth of `from` to that of the other island,
/// begun at `now`.
pub(crate) fn course(
    harbor: &Harbor,
    water: f32,
    from: IslandId,
    now: f32,
    motion: &Motion,
) -> Trip {
    let past = shapes::SEAWARD + shapes::OFFING;
    let at = |isle: IslandId, along: f32, lift: f32| {
        let p = harbor.spot(isle, along, shapes::ALONGSIDE);
        Vec3::new(p.x, water + lift, p.y)
    };
    let to = from.other();
    let mut trip = Trip::new(
        vec![
            at(from, shapes::MOORED, 0.0),
            at(from, past, OPEN_LIFT),
            at(to, past, OPEN_LIFT),
            at(to, shapes::MOORED, 0.0),
        ],
        now,
        motion,
    );
    trip.speed = SPEED;
    trip
}

/// Casts off a copy of the boat made of `model`, moored at `home`, on
/// `trip`.
pub(crate) fn launch(
    commands: &mut Commands,
    model: (&Mesh3d, &MeshMaterial3d<StandardMaterial>),
    home: IslandId,
    trip: Trip,
) -> Entity {
    commands
        .spawn((
            Ferry { home },
            model.0.clone(),
            model.1.clone(),
            Transform::from_translation(trip.way[0]),
            Visibility::Inherited,
            trip,
        ))
        .id()
}

/// On a ferry that has set down its passenger and sails home empty,
/// to be taken down when it gets there.
#[derive(Component, Clone, Copy, Debug)]
pub(super) struct Homeward;

/// Sends `ferry` on `trip` back to where it came from.
pub(crate) fn send_home(
    commands: &mut Commands,
    ferry: Entity,
    trip: Trip,
) {
    commands.entity(ferry).insert((trip, Homeward));
}

/// Moves each ferry along its trip, rocking on the swell and turning
/// to lie as a moored boat does where it ends.
pub(super) fn sail(
    mut commands: Commands,
    clock: Res<Clock>,
    time: Res<Time>,
    land: Res<Land>,
    harbor: Option<Res<Harbor>>,
    mut ferries: Query<(
        Entity,
        &Ferry,
        &Trip,
        &mut Transform,
        Has<Homeward>,
    )>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    let Some(harbor) = harbor else {
        return;
    };
    for (entity, ferry, trip, mut at, homeward) in &mut ferries {
        redraw.write(RequestRedraw);
        if homeward && trip.has_arrived(clock.t) {
            commands.entity(entity).despawn();
            continue;
        }
        let step = trip.at(clock.t);
        let end = trip.way.last().copied().unwrap_or_default();
        let moored = harbor.seaward(land.island_at(end.xz()));
        let left = trip.length() - step.gone;
        let w = (1.0 - left / TURNS_OVER).clamp(0.0, 1.0);
        let yaw = step.yaw
            + ((moored - step.yaw + PI).rem_euclid(TAU) - PI) * w;
        let (lift, tilt) = rock(ferry.home, time.elapsed_secs());
        at.translation = step.at + Vec3::Y * lift;
        at.rotation = Quat::from_rotation_y(yaw) * tilt;
    }
}

/// Hides the moored boat a ferry is a copy of while it is out, and
/// any moored boat it lies at.
pub(super) fn moor(
    ferries: Query<(&Ferry, &Transform)>,
    mut boats: Query<(&Boat, &GlobalTransform, &mut Visibility)>,
) {
    for (boat, place, mut seen) in &mut boats {
        let away = ferries.iter().any(|(ferry, at)| {
            ferry.home == boat.0
                || at
                    .translation
                    .xz()
                    .distance(place.translation().xz())
                    < BERTH
        });
        seen.set_if_neq(if away {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        });
    }
}
