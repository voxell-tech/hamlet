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

impl Status {
    /// What a plugin in this status is, in a few words.
    pub fn words(self) -> &'static str {
        match self {
            Status::Starting => "not started yet",
            Status::Running => "running",
            Status::Disabled => "switched off",
            Status::Failed => "stopped",
        }
    }
}
