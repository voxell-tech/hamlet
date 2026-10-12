//! A robot carried in the hand, and its fall when it is let go.

use bevy::picking::Pickable;
use bevy::prelude::*;
use hamlet_core::agents::{Mayor, Road};

use super::{
    Hand, Held, Mood, PlaySettings, Wish, free, liftable, met,
    toward, yaw_of,
};
use crate::Clock;
use crate::generator::land::Land;
use crate::generator::{Passage, Robot};
use crate::hamlet::Housed;
use crate::keeper::Act;
use crate::moving::Carrying;
use crate::pick::{
    Hovered, PickSettings, Reach, Taken, Thing, Travel, WorldSurface,
};
use crate::play::{Play, Played};
use crate::sky::CameraSettings;

/// A fall to the ground, and what comes after it.
#[derive(Clone, Copy, Debug)]
pub(super) struct Fall {
    speed: f32,
    /// The point of the ground it comes down on.
    to: Vec2,
    /// The robot it was set down to meet.
    meets: Option<Entity>,
}

/// The left button goes down on the world: the hand takes the robot
/// under the pointer, and the ground stays where it is.
pub(super) fn grab(
    press: On<Pointer<Press>>,
    surfaces: Query<(), With<WorldSurface>>,
    hovered: Res<Hovered>,
    boxes: Query<(Entity, &Thing, &ChildOf)>,
    robots: Query<&Robot>,
    passages: Query<&Passage>,
    carrying: Option<Res<Carrying>>,
    mut hand: ResMut<Hand>,
    mut taken: ResMut<Taken>,
) {
    if press.button != PointerButton::Primary
        || !surfaces.contains(press.event_target())
        || carrying.is_some_and(|c| c.0.is_some())
    {
        return;
    }
    // A press in the frame of a release: what is held is let go
    // first.
    if hand.held.is_some() {
        return;
    }
    hand.released = false;
    hand.held = liftable(hovered.0, &boxes, &robots, &passages).map(
        |robot| Held {
            robot,
            lifted: false,
        },
    );
    taken.drag = hand.held.is_some();
}

pub(super) fn let_go(
    release: On<Pointer<Release>>,
    mut hand: ResMut<Hand>,
) {
    if release.button == PointerButton::Primary && hand.held.is_some()
    {
        hand.released = true;
    }
}

pub(super) fn drag_end(
    end: On<Pointer<DragEnd>>,
    mut hand: ResMut<Hand>,
) {
    if end.button == PointerButton::Primary && hand.held.is_some() {
        hand.released = true;
    }
}

/// Moves what the hand holds, and lets it go when the button is.
pub(super) fn carry(
    mut commands: Commands,
    (time, set, pick, camera): (
        Res<Time>,
        Res<PlaySettings>,
        Res<PickSettings>,
        Res<CameraSettings>,
    ),
    (travel, reach, land, hovered): (
        Res<Travel>,
        Res<Reach>,
        Res<Land>,
        Res<Hovered>,
    ),
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    mut hand: ResMut<Hand>,
    mut taken: ResMut<Taken>,
    boxes: Query<(Entity, &Thing, &ChildOf)>,
    plain: Query<&Robot>,
    agents: Query<(Entity, &Housed, Has<Mayor>)>,
    roads: Query<&Road>,
    mut robots: Query<(&Robot, &mut Transform, &mut Mood)>,
    (mut played, mut wishes): (
        MessageWriter<Played>,
        MessageWriter<Wish>,
    ),
) {
    let Some(Held { robot, lifted }) = hand.held else {
        if taken.drag {
            taken.drag = false;
        }
        return;
    };
    let released = hand.released
        || mouse
            .is_some_and(|mouse| !mouse.pressed(MouseButton::Left));
    let dragged = travel.0 > camera.click_slop;
    let on_plane = |height: f32| {
        let ray = reach.ray?;
        let far = ray.intersect_plane(
            Vec3::Y * height,
            InfinitePlane3d::new(Vec3::Y),
        )?;
        Some(ray.get_point(far))
    };
    let dt = time.delta_secs();
    let Ok((of, mut at, mut mood)) = robots.get_mut(robot) else {
        hand.held = None;
        return;
    };
    let me = agents
        .iter()
        .find(|(_, housed, _)| housed.0 == of.agent)
        .map(|(agent, _, mayor)| (agent, mayor));
    if !lifted && dragged && !released {
        hand.held = Some(Held {
            robot,
            lifted: true,
        });
        mood.face = None;
        mood.joins = None;
        commands.entity(robot).insert(Passage::hold(Act::Dangle));
        // The pointer must reach what is under it.
        for (hit, _, parent) in &boxes {
            if parent.parent() == robot {
                commands.entity(hit).remove::<Pickable>();
            }
        }
        played.write(Played {
            what: Play::Lift,
            at: at.translation,
        });
    }
    if lifted && !released {
        let ground = Robot::ground(&land, at.translation.xz());
        let high = ground + set.lift_height;
        let want = on_plane(high + pick.robot_box.y)
            .map_or(at.translation.with_y(high), |p| p.with_y(high));
        let k = 1.0 - (-set.lift_follow * dt).exp();
        let was = at.translation;
        at.translation = was.lerp(want, k);
        // It swings behind the hand that carries it.
        let speed = (at.translation - was) / dt.max(1e-4);
        let yaw = Quat::from_rotation_y(yaw_of(at.rotation));
        let own = yaw.inverse() * speed;
        let lean = |v: f32| (v * set.lift_lean).clamp(-0.6, 0.6);
        let hung = yaw
            * Quat::from_rotation_x(-lean(own.z))
            * Quat::from_rotation_z(lean(own.x));
        at.rotation = at.rotation.slerp(hung, k);
    }
    let (here, home) = (at.translation, of.porch());
    if released {
        hand.held = None;
        hand.released = false;
        taken.drag = false;
        if !lifted {
            return;
        }
        let other = hovered
            .0
            .and_then(|thing| met(thing, &boxes, &plain, &agents))
            .filter(|(agent, _)| {
                me.is_some_and(|(me, _)| me != *agent)
            });
        let mut fall = Fall {
            speed: 0.0,
            to: here.xz(),
            meets: None,
        };
        if let (Some((other, their)), Some((me, mayor))) = (other, me)
        {
            let known =
                roads.iter().any(|r| r.has(me) && r.has(other));
            let office = mayor
                || agents.get(other).is_ok_and(|(.., mayor)| mayor);
            if !known && !office {
                wishes.write(Wish::Road { a: me, b: other });
            }
            // It comes down a step from the other, on the
            // side it came from.
            let them = their
                .and_then(|robot| plain.get(robot).ok())
                .map_or(here, Robot::porch);
            let them = their
                .and_then(|their| robots.get(their).ok())
                .map_or(them, |(_, place, _)| place.translation);
            let aside = (here.xz() - them.xz())
                .try_normalize()
                .or((home.xz() - them.xz()).try_normalize())
                .unwrap_or(Vec2::X);
            fall.to = them.xz() + aside * set.greet_gap;
            fall.meets = their;
        }
        if let Ok((_, _, mut mood)) = robots.get_mut(robot) {
            mood.fall = Some(fall);
        }
    }
}

/// Brings each robot that was let go down to the ground, where it
/// lands and makes for home, or greets the robot it was set down to
/// meet.
pub(super) fn fall(
    mut commands: Commands,
    time: Res<Time>,
    clock: Res<Clock>,
    set: Res<PlaySettings>,
    land: Res<Land>,
    boxes: Query<(Entity, &Thing, &ChildOf)>,
    mut robots: Query<(
        Entity,
        &mut Robot,
        &mut Transform,
        &mut Mood,
        Option<&Passage>,
    )>,
    mut played: MessageWriter<Played>,
) {
    let dt = time.delta_secs();
    let mut greeted = Vec::new();
    for (entity, mut robot, mut at, mut mood, _) in &mut robots {
        let Some(mut fall) = mood.fall else {
            continue;
        };
        fall.speed += set.fall * dt;
        let k = 1.0 - (-set.lift_follow * dt).exp();
        let over = at.translation.xz().lerp(fall.to, k);
        let ground = Robot::ground(&land, over);
        let height = at.translation.y - fall.speed * dt;
        at.translation =
            Vec3::new(over.x, height.max(ground), over.y);
        let upright = Quat::from_rotation_y(yaw_of(at.rotation));
        at.rotation = at.rotation.slerp(upright, k);
        if height > ground {
            mood.fall = Some(fall);
            continue;
        }
        mood.fall = None;
        at.rotation = upright;
        at.translation = Vec3::new(fall.to.x, ground, fall.to.y);
        let here = at.translation;
        // The longer the way home, the faster it goes, up to a
        // point.
        let far = here.distance(robot.porch());
        let haste = (far / set.scurry_longest)
            .clamp(set.scurry, set.scurry_fastest.max(set.scurry));
        robot.put_down(Some(haste));
        for (hit, _, parent) in &boxes {
            if parent.parent() == entity {
                commands.entity(hit).insert(Pickable::default());
            }
        }
        let (act, what) = match fall.meets {
            Some(_) => (Act::Greet, Play::Introduce),
            None => (Act::Land, Play::Drop),
        };
        commands.entity(entity).insert(Passage::new(act));
        played.write(Played { what, at: here });
        if let Some(other) = fall.meets {
            greeted.push((entity, other, here));
        }
    }
    for (comer, other, here) in greeted {
        let Ok((_, robot, place, mut mood, passage)) =
            robots.get_mut(other)
        else {
            continue;
        };
        let there = place.translation;
        if free(&robot, passage) && !mood.sulks(clock.t) {
            mood.joins = Some((clock.t, Act::Greet));
            mood.face = Some(toward(there, here));
        }
        if let Ok((.., mut mood, _)) = robots.get_mut(comer) {
            mood.face = Some(toward(here, there));
        }
    }
}
