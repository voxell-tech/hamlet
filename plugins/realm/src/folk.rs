//! Who lives in the realm: a robot for every agent and the chief.

pub mod chief;
pub mod hand;
pub mod keeper;
pub mod sounds;
pub mod talk;

use hamlet_engine_bevy::api::agents::AgentsRequest;
use hamlet_engine_bevy::prelude::*;

use crate::folk::chief::{Keeping, follow_company};
use crate::folk::hand::{
    Hand, Letters, Toys, hand_letters, pin_thoughts, play_hand,
};
use crate::folk::keeper::{
    Robots, ViewShape, follow_needs, follow_view, move_robots,
};
use crate::folk::sounds::FolkCue;
use crate::folk::talk::{Talk, hear_speech, keep_balloons};
use crate::places::road::build_roads;
use crate::tuning::WorldTuning;
use crate::view::camera::Camera;

pub(crate) fn plugin(app: &mut App) {
    app.add_message::<FolkCue>()
        .init_resource::<Hand>()
        .init_resource::<Toys>()
        .init_resource::<Letters>()
        .init_resource::<Talk>()
        .init_resource::<ViewShape>()
        .init_resource::<Keeping>()
        .add_systems(Startup, ask)
        .add_systems(
            Update,
            (
                follow_needs.run_if(resource_exists::<Robots>),
                follow_company.run_if(resource_exists::<Robots>),
                hand_letters.run_if(resource_exists::<Robots>),
                hear_speech.run_if(resource_exists::<Robots>),
                // The balloons hang over the robots where they
                // moved to.
                keep_balloons
                    .run_if(
                        resource_exists::<Robots>.and_then(ticked),
                    )
                    .after(move_robots),
                follow_view.run_if(resource_exists::<Camera>),
                pin_thoughts.run_if(resource_exists::<WorldTuning>),
                // The hand plays on the robots as the pointer finds
                // them, before they move.
                play_hand
                    .run_if(
                        resource_exists::<Robots>.and_then(ticked),
                    )
                    .after(build_roads)
                    .before(move_robots),
            ),
        );
}

/// True on a call that moves time on: the realm's frame.
fn ticked(time: Res<Time>) -> bool {
    !time.delta().is_zero()
}

/// Asks for what waits for the chief, which the robots show.
fn ask(mut commands: Commands) {
    commands.agents(&AgentsRequest::Needs);
}
