//! A shortcut a plugin adds.

use bevy::ecs::component::Component;
use bevy_fynix::shortcut::Chord;

/// On an entity a plugin declared: a chord that runs one of its
/// commands.
#[derive(Component, Clone, PartialEq, Eq, Debug)]
#[component(immutable)]
pub struct Key {
    pub chord: Chord,
    /// The name of a command of the same plugin.
    pub command: String,
}
