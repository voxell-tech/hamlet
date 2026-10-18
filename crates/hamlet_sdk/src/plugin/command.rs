//! A command a plugin declares, which other plugins may run.

use bevy::ecs::component::Component;

/// On an entity a plugin declared: a command it exports. The plugin
/// that owns it is the one it is
/// [`ContributedBy`](super::ContributedBy).
#[derive(Component, Clone, PartialEq, Eq, Debug)]
#[component(immutable)]
pub struct Command {
    /// The name the owner exports it by.
    pub id: String,
    /// What a menu or a key shows for it.
    pub title: String,
}
