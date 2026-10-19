//! Clicks and strokes on a robot: a poke, a pat, a cheer, and a nudge
//! of a thinking agent's light.

use std::f32::consts::PI;

use bevy::prelude::*;
use hamlet_client::realm::Busy;

use super::toys::{self, Kind, Toys};
use super::{Hand, Mood, PlaySettings, free, robot_of, toward};
use crate::bubble::Pinned;
use crate::generator::{Passage, Robot, Thought};
use crate::hamlet::Housed;
use crate::keeper::Act;
use crate::pick::{Hovered, Reach, Taken, Thing, Travel};
use crate::play::{Play, Played};
use crate::sky::CameraSettings;
use crate::{Clock, WorldCamera};

/// A robot was clicked.
#[derive(Message, Clone, Copy, Debug)]
pub(super) struct Tap(Entity);

/// On an agent whose work ended a moment ago, until that clock
/// time: a click on its robot cheers it.
#[derive(Component, Clone, Copy, Debug)]
pub struct Proud {
    until: f32,
}

/// The pointer resting on a robot, or stroking it.
#[derive(Clone, Copy, Debug)]
pub(super) struct Rest {
    robot: Entity,
    /// The spot it settled at, and the time it did.
    anchor: Vec2,
    since: f32,
    last: Vec2,
    /// The stroke so far: when it began, the pixels covered, the
    /// way it last went and how often it turned back.
    began: f32,
    path: f32,
    heading: f32,
    turns: u32,
}

/// Tells the world of a thing that was selected, or asked about.
pub(super) fn tell(
    click: On<Pointer<Click>>,
    things: Query<&GlobalTransform, With<Thing>>,
    travel: Res<Travel>,
    camera: Res<CameraSettings>,
    mut played: MessageWriter<Played>,
) {
    let Ok(place) = things.get(click.event_target()) else {
        return;
    };
    let what = match click.button {
        PointerButton::Primary => Play::Select,
        PointerButton::Secondary => Play::Menu,
        PointerButton::Middle => return,
    };
    if travel.0 <= camera.click_slop {
        played.write(Played {
            what,
            at: place.translation(),
        });
    }
}

/// A plain click of the left button on a robot.
pub(super) fn tap(
    click: On<Pointer<Click>>,
    boxes: Query<(&Thing, &ChildOf)>,
    robots: Query<(), With<Robot>>,
    travel: Res<Travel>,
    taken: Res<Taken>,
    camera: Res<CameraSettings>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mut taps: MessageWriter<Tap>,
) {
    let Ok((thing, parent)) = boxes.get(click.event_target()) else {
        return;
    };
    let shift = keys.is_some_and(|keys| {
        keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight])
    });
    if click.button == PointerButton::Primary
        && !shift
        && !taken.click
        && travel.0 <= camera.click_slop
        && matches!(thing, Thing::Keeper | Thing::Resident(_))
        && robots.contains(parent.parent())
    {
        taps.write(Tap(parent.parent()));
    }
}

/// A click on the light over a thinking agent's house holds its
/// thought open in its balloon.
pub(super) fn nudge(
    click: On<Pointer<Click>>,
    set: Res<PlaySettings>,
    thoughts: Query<(&Thought, &GlobalTransform)>,
    agents: Query<(Entity, &Housed)>,
    mut commands: Commands,
    mut played: MessageWriter<Played>,
) {
    let Ok((thought, place)) = thoughts.get(click.event_target())
    else {
        return;
    };
    if click.button != PointerButton::Primary {
        return;
    }
    let Some((agent, _)) =
        agents.iter().find(|(_, housed)| housed.0 == thought.of)
    else {
        return;
    };
    commands.entity(agent).insert(Pinned(set.nudge_pin));
    played.write(Played {
        what: Play::Nudge,
        at: place.translation(),
    });
}

/// Gives the light of each new thought the room a pointer can hit.
pub(super) fn room(
    set: Res<PlaySettings>,
    mut thoughts: Query<&mut Transform, Added<Thought>>,
) {
    for mut at in &mut thoughts {
        at.scale = Vec3::splat(set.nudge_room);
    }
}

/// An agent's work ended: for a while its robot can be cheered.
pub(super) fn finished(
    done: On<Remove, Busy>,
    clock: Res<Clock>,
    set: Res<PlaySettings>,
    mut commands: Commands,
) {
    if let Ok(mut agent) = commands.get_entity(done.entity) {
        agent.try_insert(Proud {
            until: clock.t + set.cheer_window,
        });
    }
}

/// Lets the time for a cheer run out.
pub(super) fn fade(
    mut commands: Commands,
    clock: Res<Clock>,
    proud: Query<(Entity, &Proud, Has<Busy>)>,
) {
    for (agent, proud, busy) in &proud {
        if busy || clock.t >= proud.until {
            commands.entity(agent).remove::<Proud>();
        }
    }
}

/// Answers each click on a robot: a cheer for one whose work has
/// just ended, and a poke for the second click of two.
pub(super) fn pokes(
    mut commands: Commands,
    clock: Res<Clock>,
    set: Res<PlaySettings>,
    toys: Option<Res<Toys>>,
    mut taps: MessageReader<Tap>,
    mut hand: ResMut<Hand>,
    eye: Query<&GlobalTransform, With<WorldCamera>>,
    agents: Query<(Entity, &Housed, Has<Proud>)>,
    mut robots: Query<(
        Entity,
        &Robot,
        &Transform,
        &mut Mood,
        Option<&Passage>,
    )>,
    mut played: MessageWriter<Played>,
) {
    let now = clock.t;
    if clock.frozen {
        taps.clear();
        return;
    }
    let eye = eye.iter().next().map(GlobalTransform::translation);
    for Tap(tapped) in taps.read().copied() {
        let Ok((_, robot, place, mut mood, passage)) =
            robots.get_mut(tapped)
        else {
            continue;
        };
        let at = place.translation;
        if !free(robot, passage) || mood.sulks(now) {
            continue;
        }
        let proud = agents
            .iter()
            .find(|(_, housed, proud)| {
                housed.0 == robot.agent && *proud
            })
            .map(|(agent, ..)| agent);
        if let Some(agent) = proud {
            commands.entity(agent).remove::<Proud>();
            hand.last_tap = None;
            played.write(Played {
                what: Play::Cheer,
                at,
            });
            if let Some(toys) = &toys {
                let top = at + Vec3::Y * set.heart_height;
                for i in 0..3 {
                    let side = Vec3::X * (i as f32 - 1.0) * 0.3;
                    toys::toss(
                        &mut commands,
                        toys,
                        Kind::Spark,
                        now + i as f32 * 0.12,
                        (top + side, top + side * 2.0 + Vec3::Y),
                        i as f32,
                    );
                }
            }
            // Its neighbours join in, the nearest first.
            for (entity, robot, place, mut mood, passage) in
                &mut robots
            {
                let far = place.translation.distance(at);
                if entity == tapped {
                    mood.joins = Some((now, Act::Celebrate));
                } else if far < set.cheer_reach
                    && free(robot, passage)
                    && !mood.sulks(now)
                {
                    let wait = 0.25 + far * set.cheer_lag;
                    mood.joins = Some((now + wait, Act::Celebrate));
                }
            }
            continue;
        }
        let second = hand.last_tap.is_some_and(|(robot, then)| {
            robot == tapped && now - then <= set.poke_gap
        });
        if !second {
            hand.last_tap = Some((tapped, now));
            continue;
        }
        hand.last_tap = None;
        mood.pokes.retain(|then| now - then <= set.annoy_window);
        mood.pokes.push(now);
        let to_eye = eye.map(|eye| toward(at, eye));
        if mood.pokes.len() >= set.annoy_pokes {
            // It turns its back on whoever keeps poking it.
            mood.pokes.clear();
            mood.sulks_until =
                now + Act::Sulk.length().unwrap_or_default();
            mood.face = to_eye.map(|yaw| yaw + PI);
            commands.entity(tapped).insert(Passage::new(Act::Sulk));
            played.write(Played {
                what: Play::Annoy,
                at,
            });
        } else {
            mood.face = to_eye;
            commands
                .entity(tapped)
                .insert(Passage::new(Act::Startle));
            played.write(Played {
                what: Play::Poke,
                at,
            });
        }
    }
}

/// Pats the robot the pointer rests on, or strokes back and forth.
pub(super) fn pats(
    mut commands: Commands,
    clock: Res<Clock>,
    set: Res<PlaySettings>,
    toys: Option<Res<Toys>>,
    hovered: Res<Hovered>,
    reach: Res<Reach>,
    mut hand: ResMut<Hand>,
    boxes: Query<(Entity, &Thing, &ChildOf)>,
    plain: Query<&Robot>,
    busy: Query<(&Housed, Has<Busy>)>,
    mut robots: Query<(
        &Robot,
        &Transform,
        &mut Mood,
        Option<&Passage>,
    )>,
    mut played: MessageWriter<Played>,
) {
    let now = clock.t;
    let on = hovered
        .0
        .filter(|_| hand.held.is_none() && !clock.frozen)
        .and_then(|thing| robot_of(thing, &boxes, &plain));
    let (Some(on), Some(at)) = (on, reach.at) else {
        hand.rest = None;
        return;
    };
    let fresh = Rest {
        robot: on,
        anchor: at,
        since: now,
        last: at,
        began: now,
        path: 0.0,
        heading: 0.0,
        turns: 0,
    };
    let mut rest =
        hand.rest.filter(|rest| rest.robot == on).unwrap_or(fresh);
    if rest.anchor.distance(at) > set.pat_still {
        rest.anchor = at;
        rest.since = now;
    }
    let step = at - rest.last;
    rest.last = at;
    if now - rest.began > set.stroke_time {
        (rest.began, rest.path, rest.turns) = (now, 0.0, 0);
    }
    rest.path += step.length();
    if step.x.abs() > 0.5 {
        if rest.heading * step.x < 0.0 {
            rest.turns += 1;
        }
        rest.heading = step.x.signum();
    }
    let Ok((robot, place, mut mood, passage)) = robots.get_mut(on)
    else {
        hand.rest = None;
        return;
    };
    let since = now - mood.patted;
    let held =
        now - rest.since >= set.pat_hold && since >= set.pat_rest;
    let stroked = rest.path >= set.stroke_length
        && rest.turns >= set.stroke_turns
        && since >= set.stroke_rest;
    if (held || stroked) && free(robot, passage) && !mood.sulks(now) {
        mood.patted = now;
        mood.face = None;
        (rest.began, rest.path, rest.turns) = (now, 0.0, 0);
        rest.since = now;
        commands.entity(on).insert(Passage::new(Act::Wiggle));
        let at = place.translation;
        played.write(Played {
            what: Play::Pat,
            at,
        });
        // A heart for one at rest, a spark for one at work.
        let working = busy
            .iter()
            .any(|(housed, busy)| housed.0 == robot.agent && busy);
        if let Some(toys) = &toys {
            let from = at + Vec3::Y * set.heart_height;
            toys::toss(
                &mut commands,
                toys,
                if working { Kind::Spark } else { Kind::Heart },
                now,
                (from, from + Vec3::Y * set.heart_rise),
                now,
            );
        }
    }
    hand.rest = Some(rest);
}
