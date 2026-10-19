//! Telling the chief that a command did not go as asked.

use bevy::ecs::event::Event;

/// Words for the chief, put in the open chat as a notice.
#[derive(Event, Clone, PartialEq, Eq, Debug)]
pub struct Notice(pub String);
