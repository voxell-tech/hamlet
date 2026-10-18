//! What the chief sees of an installed plugin's reloads, and the
//! question to go back to its last good version.

use bevy::ecs::component::Component;
use bevy::ecs::event::Event;

/// On the entity of an installed plugin: how its last start or reload
/// went.
#[derive(Component, Clone, PartialEq, Eq, Default, Debug)]
#[component(immutable)]
pub struct Reload {
    /// Why the version on disk is not the one running, or what it
    /// waits for.
    pub note: Option<String>,
    /// Whether a last good version differs from the files on disk.
    pub rollback: bool,
}

/// The chief wants the plugin of this id back at its last good
/// version.
#[derive(Event, Clone, PartialEq, Eq, Debug)]
pub struct Rollback {
    pub plugin: String,
}
