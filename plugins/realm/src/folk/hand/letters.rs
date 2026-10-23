//! Files dropped on the view, handed to the agent whose robot or
//! house the pointer is on.

use hamlet_engine_bevy::api::agents::AgentsRequest;
use hamlet_engine_bevy::prelude::*;

use crate::bridge::Bridge;
use crate::folk::hand::agent_of;
use crate::folk::keeper::{Act, Passage, Robots};
use crate::realm::Clock;
use crate::sound::{Cue, Sound};
use crate::tuning::WorldTuning;
use crate::view::pick::Picking;

/// The files dropped that no agent has been handed yet, by the
/// engine's number for each, and the seconds each waits for the
/// pointer to be seen on an agent.
#[derive(Resource, Default)]
pub struct Letters(Vec<(u32, f32)>);

/// Hands each file dropped to the agent under the pointer. One that
/// fell on nothing is let lie once its wait is over.
pub fn hand_letters(
    mut dropped: MessageReader<SceneDrop>,
    mut letters: ResMut<Letters>,
    tuning: Res<WorldTuning>,
    clock: Res<Clock>,
    picking: Res<Picking>,
    bridge: Res<Bridge>,
    mut robots: ResMut<Robots>,
    mut commands: Commands,
    mut cues: MessageWriter<Cue>,
) {
    for drop in dropped.read() {
        letters.0.push((drop.id, tuning.play.letter_wait));
    }
    if letters.0.is_empty() {
        return;
    }
    // The spot it fell on is known once the pointer is seen on
    // something.
    let to = agent_of(picking.hovered());
    let Some((agent, name)) =
        to.and_then(|agent| Some((agent, bridge.named(agent)?)))
    else {
        letters.0.retain_mut(|(_, left)| {
            *left -= clock.dt;
            *left > 0.0
        });
        return;
    };
    for (i, (id, _)) in letters.0.drain(..).enumerate() {
        commands.agents(&AgentsRequest::Hand {
            agent: name.0.clone(),
            drop: id,
        });
        let Some(robot) = robots.robot_mut(agent) else {
            continue;
        };
        cues.write(Cue {
            sound: Sound::Hand,
            at: robot.rig.at.at,
            turn: agent.0,
        });
        if robot.free() && i == 0 {
            robot.passage =
                Some(Passage::new(Act::Receive, clock.now));
        }
    }
}
