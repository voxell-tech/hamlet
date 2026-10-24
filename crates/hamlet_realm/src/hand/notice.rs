//! Robots that notice the pointer: a head turned to it, a wave, a
//! motion joined in, and the way one faces while it plays.

use bevy::prelude::*;
use hamlet_vfx::magic::chance;

use super::{
    Hand, Mood, PlaySettings, bend, is_free, toward, yaw_of,
};
use crate::generator::{Passage, Robot};
use crate::keeper::{Act, Keeper, Motion};
use crate::pick::Reach;
use crate::play::{Play, Played};
use crate::{Clock, WorldCamera};

/// Turns each robot's head to the pointer as it passes, and has one
/// at rest wave now and then.
pub(super) fn watch(
    time: Res<Time>,
    clock: Res<Clock>,
    set: Res<PlaySettings>,
    reach: Res<Reach>,
    hand: Res<Hand>,
    eye: Query<&GlobalTransform, With<WorldCamera>>,
    mut commands: Commands,
    mut robots: Query<(
        Entity,
        &Robot,
        &Transform,
        &mut Keeper,
        &mut Mood,
        Option<&Passage>,
    )>,
    mut played: MessageWriter<Played>,
) {
    let now = clock.t;
    let k = 1.0 - (-set.look_rate * time.delta_secs()).exp();
    let eye = eye.iter().next().map(GlobalTransform::translation);
    for (entity, robot, place, mut keeper, mut mood, passage) in
        &mut robots
    {
        let here = place.translation;
        let idle = passage.is_none()
            && robot.is_at_house()
            && hand.held.is_none()
            && !clock.frozen;
        // The pointer, at the height of its head.
        let pointer = reach.ray.filter(|_| idle).and_then(|ray| {
            let far = ray.intersect_plane(
                here + Vec3::Y,
                InfinitePlane3d::new(Vec3::Y),
            )?;
            Some(ray.get_point(far))
        });
        let far = pointer.map(|p| p.xz().distance(here.xz()));
        let want = match (pointer, far) {
            (Some(pointer), Some(far)) if far < set.look_near => {
                let turn = bend(
                    yaw_of(place.rotation),
                    toward(here, pointer),
                )
                .clamp(-set.look_turn, set.look_turn);
                let lift =
                    set.look_lift * (1.0 - far / set.look_near);
                Vec2::new(turn, lift)
            }
            _ => Vec2::ZERO,
        };
        let look = mood.look.lerp(want, k);
        let look = if look.length() < 1e-3 {
            Vec2::ZERO
        } else {
            look
        };
        if mood.look != look {
            mood.look = look;
            keeper.look(look.x, look.y);
        }
        let near = far.is_some_and(|far| far < set.wave_near);
        if near && !mood.near {
            mood.passes += 1;
            let seed = keeper.who().seed as f32;
            let lucky = chance(seed * 7.3 + mood.passes as f32)
                < set.wave_chance;
            if lucky
                && keeper.act() == Act::Idle
                && now - mood.waved >= set.wave_rest
                && !mood.is_sulking(now)
            {
                mood.waved = now;
                mood.face = eye.map(|eye| toward(here, eye));
                commands
                    .entity(entity)
                    .insert(Passage::new(Act::Wave));
                played.write(Played {
                    what: Play::Wave,
                    at: here,
                });
            }
        }
        if mood.near != near {
            mood.near = near;
        }
    }
}

/// Starts the motion each robot was to join in when its time comes.
pub(super) fn join(
    mut commands: Commands,
    clock: Res<Clock>,
    mut robots: Query<(Entity, &Robot, &mut Mood, Option<&Passage>)>,
) {
    for (entity, robot, mut mood, passage) in &mut robots {
        let Some((at, act)) = mood.joins else {
            continue;
        };
        if clock.t < at {
            continue;
        }
        mood.joins = None;
        if is_free(robot, passage) {
            commands.entity(entity).insert(Passage::new(act));
        }
    }
}

/// Turns each robot the way its passage has it face.
pub(super) fn face(
    time: Res<Time>,
    motion: Res<Motion>,
    hand: Res<Hand>,
    mut robots: Query<(
        Entity,
        &mut Transform,
        &mut Mood,
        Has<Passage>,
    )>,
) {
    let k = 1.0 - (-motion.turn_rate * 0.5 * time.delta_secs()).exp();
    for (entity, mut at, mut mood, playing) in &mut robots {
        let Some(yaw) = mood.face else {
            continue;
        };
        if !playing && mood.joins.is_none() {
            mood.face = None;
            continue;
        }
        if hand.lifted() == Some(entity) || mood.fall.is_some() {
            continue;
        }
        let to = Quat::from_rotation_y(yaw);
        if at.rotation != to {
            at.rotation = at.rotation.slerp(to, k);
        }
    }
}
