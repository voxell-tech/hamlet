//! The set on show: its cast, their scripts and the camera on them.

use bevy::prelude::*;
use hamlet_world::keeper::{
    Act, Keeper, Kit, Motion, Trip, spawn_agent,
};
use hamlet_world::{Clock, WorldCamera};

use crate::sets;
use crate::style::scene::Terrain;

#[derive(Resource)]
pub struct Sets(pub Vec<sets::Set>);

/// The set on show, if any.
#[derive(Resource)]
pub struct Show {
    pub set: Option<usize>,
    /// Its cast has yet to be spawned.
    pub fresh: bool,
    /// True when the cast's lanterns light the ground.
    pub lights: bool,
}

/// One of a set's cast.
#[derive(Component)]
pub struct Extra;

/// The motions an extra has yet to play, each with its clock time.
#[derive(Component)]
pub struct Script {
    lines: Vec<(f32, Act)>,
    next: usize,
}

/// The middle of every set: open grass in front of the stall.
const SET_AT: Vec3 = Vec3::new(1.3, 0.0, 4.6);

/// Swaps the cast when the set changes.
pub fn cast(
    mut commands: Commands,
    mut show: ResMut<Show>,
    sets: Res<Sets>,
    mut kit: ResMut<Kit>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    terrain: Res<Terrain>,
    motion: Res<Motion>,
    extras: Query<Entity, With<Extra>>,
) {
    if !show.fresh {
        return;
    }
    show.fresh = false;
    for e in &extras {
        commands.entity(e).despawn();
    }
    let Some(set) = show.set.map(|i| &sets.0[i]) else {
        return;
    };
    kit.lamps_cast_light = show.lights;
    let ground = |p: Vec2| {
        let (x, z) = (SET_AT.x + p.x, SET_AT.z + p.y);
        Vec3::new(x, terrain.ground_y(x, z), z)
    };
    for actor in &set.cast {
        let at = Transform::from_translation(ground(actor.at))
            .with_rotation(Quat::from_rotation_y(actor.yaw));
        let e = spawn_agent(
            &mut commands,
            &kit,
            &mut materials,
            actor.who,
            at,
            actor.born,
        );
        commands.entity(e).insert((
            Extra,
            Script {
                lines: actor.script.clone(),
                next: 0,
            },
        ));
        if let Some((way, depart)) = &actor.trip {
            let way = std::iter::once(actor.at)
                .chain(way.iter().copied())
                .map(ground)
                .collect::<Vec<_>>();
            let mut trip = Trip::new(way, *depart, &motion);
            if let Some(haste) = actor.haste {
                trip.speed = haste;
            }
            commands.entity(e).insert(trip);
        }
    }
}

/// Plays each extra's script up to the present, and keeps the
/// scene's own robots out of a set.
pub fn direct(
    clock: Res<Clock>,
    show: Res<Show>,
    mut extras: Query<(&mut Keeper, &mut Script)>,
    mut others: Query<
        &mut Visibility,
        (With<Keeper>, Without<Extra>),
    >,
) {
    for (mut keeper, mut script) in &mut extras {
        while let Some(&(at, act)) = script.lines.get(script.next)
            && at <= clock.t
        {
            keeper.play(act, at);
            script.next += 1;
        }
    }
    let want = if show.set.is_some() {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    };
    for mut v in &mut others {
        if *v != want {
            *v = want;
        }
    }
}

/// Points the camera at the set on show.
pub fn aim(
    show: Res<Show>,
    sets: Res<Sets>,
    mut camera: Single<&mut Transform, With<WorldCamera>>,
) {
    let Some(set) = show.set.map(|i| &sets.0[i]) else {
        return;
    };
    let focus = SET_AT + set.focus;
    let pitch = set.pitch.to_radians();
    let back = Vec3::new(
        set.orbit.sin() * pitch.cos(),
        pitch.sin(),
        set.orbit.cos() * pitch.cos(),
    );
    **camera = Transform::from_translation(focus + set.dist * back)
        .looking_at(focus, Vec3::Y);
}
