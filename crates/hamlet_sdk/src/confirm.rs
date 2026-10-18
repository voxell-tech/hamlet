//! Asking the chief before a command is run.

use bevy::ecs::event::Event;

/// Trigger it to ask the chief a question at the pointer. When the
/// chief says yes, `command` is given as the chief's own, as a
/// button of the window gives one.
#[derive(Event, Clone, PartialEq)]
pub struct Confirm {
    /// The question, in capitals.
    pub question: String,
    /// What follows if the chief says yes.
    pub outcome: String,
    /// The lettering of the button that says yes.
    pub verb: String,
    /// The chief's command to run, with its slash.
    pub command: String,
}
