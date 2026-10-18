//! The words of the status bar, for the panels that quote it.

use bevy::app::App;
use bevy::ecs::resource::Resource;
use bevy_fynix::tokens::Tone;

pub fn plugin(app: &mut App) {
    app.init_resource::<Bar>();
}

/// The words of the status bar.
#[derive(Resource, Default, PartialEq)]
pub struct Bar {
    /// The open chat's agent has a turn running.
    pub busy: bool,
    pub running: String,
    pub running_tone: Tone,
    pub model: String,
    pub link: String,
    /// Tokens and cost (or model time) of the current session.
    pub usage: String,
    /// The hamlet's spend against its budget.
    pub budget: String,
}
