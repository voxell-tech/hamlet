//! Robots that notice the pointer: a head turned to it, a wave, a
//! motion joined in, and the way one faces while it plays.

use glam::{Quat, Vec2, Vec3, Vec3Swizzles};

use crate::folk::chief::chance;
use crate::folk::hand::{
    Hand, HandFrame, bend, on_level, toward, yaw_of,
};
use crate::folk::keeper::{Act, Motion, Passage, Robots};
use crate::folk::sounds::FolkSound;

/// Turns each robot's head to the pointer as it passes, and has one
/// at rest wave now and then.
pub(super) fn watch(
    frame: &HandFrame,
    hand: &Hand,
    robots: &mut Robots,
    cue: &mut impl FnMut(FolkSound, Vec3, u32),
) {
    let (set, now) = (frame.set, frame.now);
    let k = 1.0 - (-set.look_rate * frame.dt).exp();
    for robot in robots.each_mut() {
        let here = robot.rig.at.at;
        let idle = robot.passage.is_none()
            && robot.at_home()
            && hand.held.is_none();
        // The pointer, at the height of its head.
        let pointer =
            on_level(frame.ray.filter(|_| idle), here.y + 1.0);
        let far = pointer.map(|p| p.xz().distance(here.xz()));
        let want = match (pointer, far) {
            (Some(pointer), Some(far)) if far < set.look_near => {
                let turn = bend(
                    yaw_of(robot.rig.at.turn),
                    toward(here, pointer),
                )
                .clamp(-set.look_turn, set.look_turn);
                let lift =
                    set.look_lift * (1.0 - far / set.look_near);
                Vec2::new(turn, lift)
            }
            _ => Vec2::ZERO,
        };
        let mood = &mut robot.mood;
        let look = mood.look.lerp(want, k);
        let look = if look.length() < 1e-3 {
            Vec2::ZERO
        } else {
            look
        };
        if mood.look != look {
            mood.look = look;
            robot.rig.look(look.x, look.y);
        }
        let near = far.is_some_and(|far| far < set.wave_near);
        if near && !mood.near {
            mood.passes += 1;
            let seed = robot.rig.who().seed as f32;
            let lucky = chance(seed * 7.3 + mood.passes as f32)
                < set.wave_chance;
            if lucky
                && robot.rig.act() == Act::Idle
                && now - mood.waved >= set.wave_rest
                && !mood.sulks(now)
            {
                mood.waved = now;
                mood.face = Some(toward(here, frame.eye));
                robot.passage = Some(Passage::new(Act::Wave, now));
                cue(FolkSound::Wave, here, robot.agent.0);
            }
        }
        robot.mood.near = near;
    }
}

/// Starts the motion each robot was to join in when its time comes.
pub(super) fn join(frame: &HandFrame, robots: &mut Robots) {
    for robot in robots.each_mut() {
        let Some((at, act)) = robot.mood.joins else {
            continue;
        };
        if frame.now < at {
            continue;
        }
        robot.mood.joins = None;
        if robot.free() {
            robot.passage = Some(Passage::new(act, frame.now));
        }
    }
}

/// Turns each robot the way its passage has it face.
pub(super) fn face(
    frame: &HandFrame,
    motion: &Motion,
    hand: &Hand,
    robots: &mut Robots,
) {
    let k = 1.0 - (-motion.turn_rate * 0.5 * frame.dt).exp();
    for robot in robots.each_mut() {
        let Some(yaw) = robot.mood.face else {
            continue;
        };
        if robot.passage.is_none() && robot.mood.joins.is_none() {
            robot.mood.face = None;
            continue;
        }
        if hand.lifted() == Some(robot.agent)
            || robot.mood.fall.is_some()
        {
            continue;
        }
        let to = Quat::from_rotation_y(yaw);
        let at = &mut robot.rig.at;
        if at.turn != to {
            at.turn = at.turn.slerp(to, k);
        }
    }
}
