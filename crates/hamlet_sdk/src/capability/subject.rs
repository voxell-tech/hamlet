//! What a plugin's petition is about.

use bevy::ecs::component::Component;

use super::name::Capability;

/// On the petition of a plugin in the chief's queue: what the chief
/// is being asked.
#[derive(Component, Clone, PartialEq, Eq, Debug)]
pub enum Subject {
    /// Whether to install the plugin of this id.
    Install { plugin: String },
    /// Whether to let the installed plugin of this id use what a new
    /// version asks for.
    Update {
        plugin: String,
        capabilities: Vec<Capability>,
    },
    /// Whether to let the plugin of this id use a capability it tried
    /// to use, for the target of a scoped one.
    Use {
        plugin: String,
        capability: Capability,
        target: Option<String>,
    },
    /// Whether to remove the plugin of this id, and its data.
    Remove { plugin: String },
}
