//! File drops offered to the server, and gifts and introductions in
//! the world.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::window::FileDragAndDrop;
use hamlet_client::realm::{Deeds, Mayor};

use super::toys::{self, Kind, Toys};
use super::{Hand, Mood, PlaySettings, Wish, free, met, toward};
use crate::Clock;
use crate::generator::{Passage, Robot};
use crate::hamlet::Housed;
use crate::keeper::Act;
use crate::pick::{Hovered, Thing};
use crate::play::{Play, Played};

/// The clocks, tuning, toys and deeds a letter is handed over with.
#[derive(SystemParam)]
pub(super) struct Post<'w> {
    time: Res<'w, Time>,
    clock: Res<'w, Clock>,
    set: Res<'w, PlaySettings>,
    toys: Option<Res<'w, Toys>>,
    deeds: Option<Res<'w, Deeds>>,
}

/// The thing under the pointer, and the robots and agents a letter
/// may go to.
#[derive(SystemParam)]
pub(super) struct Addressees<'w, 's> {
    hovered: Res<'w, Hovered>,
    boxes: Query<'w, 's, (Entity, &'static Thing, &'static ChildOf)>,
    plain: Query<'w, 's, &'static Robot>,
    agents: Query<'w, 's, (Entity, &'static Housed, Has<Mayor>)>,
    places: Query<
        'w,
        's,
        (&'static Transform, Option<&'static Passage>),
        With<Robot>,
    >,
}

/// The messages a letter handed over writes: the wish to give it, and
/// what is played.
#[derive(SystemParam)]
pub(super) struct Writers<'w> {
    played: MessageWriter<'w, Played>,
    wishes: MessageWriter<'w, Wish>,
}

/// Hands each file dropped on the window to the agent whose robot
/// or house the pointer is on. One that fell on nothing is let lie.
pub(super) fn letters(
    mut commands: Commands,
    post: Post,
    addressees: Addressees,
    writers: Writers,
    mut dropped: MessageReader<FileDragAndDrop>,
    mut hand: ResMut<Hand>,
) {
    let Post {
        time,
        clock,
        set,
        toys,
        deeds,
    } = post;
    let Addressees {
        hovered,
        boxes,
        plain,
        agents,
        places,
    } = addressees;
    let Writers {
        mut played,
        mut wishes,
    } = writers;
    if !deeds.is_some_and(|deeds| deeds.has("give")) {
        dropped.clear();
        hand.letters.clear();
        return;
    }
    for drop in dropped.read() {
        if let FileDragAndDrop::DroppedFile { path_buf, .. } = drop {
            hand.letters.push((path_buf.clone(), set.letter_wait));
        }
    }
    if hand.letters.is_empty() {
        return;
    }
    // The spot it fell on is known once the pointer is seen on
    // something.
    let to = hovered
        .0
        .and_then(|thing| met(thing, &boxes, &plain, &agents));
    let Some((to, robot)) = to else {
        let dt = time.delta_secs();
        hand.letters.retain_mut(|(_, left)| {
            *left -= dt;
            *left > 0.0
        });
        return;
    };
    let caught = robot.and_then(|robot| {
        let (place, passage) = places.get(robot).ok()?;
        let of = plain.get(robot).ok()?;
        Some((robot, place.translation, free(of, passage)))
    });
    for (i, (path, _)) in hand.letters.drain(..).enumerate() {
        wishes.write(Wish::Letter { to, path });
        let Some((robot, at, free)) = caught else {
            continue;
        };
        played.write(Played {
            what: Play::Letter,
            at,
        });
        if free && i == 0 {
            commands.entity(robot).insert(Passage::new(Act::Receive));
        }
        if let Some(toys) = &toys {
            let hand = at + Vec3::new(0.0, 0.9, 0.3);
            toys::toss(
                &mut commands,
                toys,
                Kind::Letter,
                clock.t,
                (hand + Vec3::Y * set.drop_height, hand),
                i as f32,
            );
        }
    }
}

/// The robot of `agent`, with where it stands.
fn robot_for(
    world: &mut World,
    agent: Entity,
) -> Option<(Entity, Vec3)> {
    let id = world.get::<Housed>(agent)?.0;
    world
        .query::<(Entity, &Robot, &Transform)>()
        .iter(world)
        .find(|(_, robot, _)| robot.agent == id)
        .map(|(entity, _, place)| (entity, place.translation))
}

/// Starts `act` on the robot of `agent` if it can play it now.
/// Returns the robot and where it stands.
fn cue(
    world: &mut World,
    agent: Entity,
    act: Act,
) -> Option<(Entity, Vec3)> {
    let (robot, at) = robot_for(world, agent)?;
    let free = world
        .get::<Robot>(robot)
        .is_some_and(|of| free(of, world.get::<Passage>(robot)));
    let now = world.resource::<Clock>().t;
    if free && let Some(mut mood) = world.get_mut::<Mood>(robot) {
        mood.joins = Some((now, act));
    }
    Some((robot, at))
}

/// Shows a skill coming to `agent`: a crate lands in its yard, and
/// its robot is glad of it.
pub fn gifted(world: &mut World, agent: Entity) {
    let Some((robot, at)) = cue(world, agent, Act::Wiggle) else {
        return;
    };
    let set = world.resource::<PlaySettings>().clone();
    let to = world
        .get::<Robot>(robot)
        .map_or(at, |robot| robot.by_porch(set.gift_at));
    let now = world.resource::<Clock>().t;
    world.try_resource_scope(|world, toys: Mut<Toys>| {
        let from = to + Vec3::Y * set.drop_height;
        let mut commands = world.commands();
        toys::toss(
            &mut commands,
            &toys,
            Kind::Gift,
            now,
            (from, to),
            now,
        );
    });
    world.write_message(Played {
        what: Play::Gift,
        at: to,
    });
    world.flush();
}

/// Has the robots of two agents turn to each other and greet, where
/// they stand.
pub fn greet(world: &mut World, a: Entity, b: Entity) {
    let (Some((robot_a, at_a)), Some((robot_b, at_b))) =
        (cue(world, a, Act::Greet), cue(world, b, Act::Greet))
    else {
        return;
    };
    for (robot, yaw) in
        [(robot_a, toward(at_a, at_b)), (robot_b, toward(at_b, at_a))]
    {
        if let Some(mut mood) = world.get_mut::<Mood>(robot)
            && mood.joins.is_some()
        {
            mood.face = Some(yaw);
        }
    }
    world.write_message(Played {
        what: Play::Introduce,
        at: at_a.midpoint(at_b),
    });
}
