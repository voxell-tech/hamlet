//! Settings pages, each an entity, as plugins add them.

use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use hamlet_engine_api::PanelId;

use crate::AnyView;

/// A page of settings.
#[derive(Component, Clone, Debug)]
#[component(immutable)]
pub struct SettingsPage {
    pub id: PanelId,
    pub title: String,
    /// Builds its content, given the page's entity.
    pub content: fn(Entity) -> AnyView,
}
