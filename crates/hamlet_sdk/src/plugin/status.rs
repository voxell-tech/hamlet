//! Where a plugin stands.

use bevy::ecs::component::Component;

/// On the entity of a plugin, replaced by inserting it again.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
#[component(immutable)]
pub enum Status {
    /// Its entry has not finished.
    Starting,
    Running,
    /// Switched off: nothing it added is there.
    Disabled,
    Failed,
}
