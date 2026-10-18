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
}

impl Subject {
    /// The id of the plugin it is about.
    pub fn plugin(&self) -> &str {
        match self {
            Subject::Install { plugin }
            | Subject::Update { plugin, .. } => plugin,
        }
    }
}
