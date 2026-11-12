//! Introductions in the world.

use bevy::prelude::*;

use super::{Mood, is_free, toward};
use crate::Clock;
use crate::generator::{Passage, Robot};
use crate::hamlet::Housed;
use crate::keeper::Act;
use crate::play::{Play, Played};

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
        .is_some_and(|of| is_free(of, world.get::<Passage>(robot)));
    let now = world.resource::<Clock>().t;
    if free && let Some(mut mood) = world.get_mut::<Mood>(robot) {
        mood.joins = Some((now, act));
    }
    Some((robot, at))
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
