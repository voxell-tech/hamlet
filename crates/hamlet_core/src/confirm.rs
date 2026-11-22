//! Asking the chief to confirm an action before it is done.

use bevy::ecs::event::Event;
use bevy::ecs::world::World;

/// An action the chief confirms before it is done: asked at the
/// pointer as `question`, with `outcome` under it and `yes` on the
/// button that does it.
#[derive(Event, Clone, Debug)]
pub struct Confirm {
    pub question: &'static str,
    pub outcome: &'static str,
    pub yes: &'static str,
    pub act: fn(&mut World),
}

impl PartialEq for Confirm {
    fn eq(&self, other: &Self) -> bool {
        // Two of the same words ask the same; what they do is not
        // compared.
        (self.question, self.outcome, self.yes)
            == (other.question, other.outcome, other.yes)
    }
}
