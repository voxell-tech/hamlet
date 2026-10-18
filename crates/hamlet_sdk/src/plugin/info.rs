//! A plugin as an entity.

use bevy::ecs::component::Component;

use super::Manifest;

/// On the entity of a plugin: what the Plugins page lists.
#[derive(Component, Clone, PartialEq, Debug)]
#[component(immutable)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub builtin: bool,
}

impl From<&Manifest> for PluginInfo {
    fn from(manifest: &Manifest) -> Self {
        Self {
            id: manifest.id.clone(),
            name: manifest.name.clone(),
            version: manifest.version.clone(),
            builtin: manifest.builtin,
        }
    }
}
