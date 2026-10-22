//! Where what a plugin adds stands among what the others add.

use bevy::ecs::component::Component;
use hamlet_engine_api::PluginId;

/// On what a plugin adds: its plugin's place in the order plugins
/// start, by the depth of its dependencies and then its id. Status
/// items, menu entries and settings pages are listed by it, after
/// the engine's own, and then in the order they came.
#[derive(Component, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[component(immutable)]
pub struct StartRank {
    pub depth: u32,
    pub plugin: PluginId,
}
