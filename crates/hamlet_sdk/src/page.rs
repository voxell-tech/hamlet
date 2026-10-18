//! A page of a host that a plugin contributes: a tab of the
//! inspector, or a page of the settings.

use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;

use crate::{AnyView, tree};

/// On a contribution of a built-in plugin that fills a page: the view
/// of the page. Without it the page is the plugin panel the
/// contribution names.
#[derive(Component, Clone, Copy)]
#[component(immutable)]
pub struct Native(pub fn() -> AnyView);

/// On a contribution of a built-in plugin that fills a page: the page
/// is there only while this holds.
#[derive(Component, Clone, Copy)]
#[component(immutable)]
pub struct ShownWhen(pub fn(&World) -> bool);

/// Whether the page of the contribution `contribution` is there.
pub fn shown(world: &World, contribution: Entity) -> bool {
    world
        .get::<ShownWhen>(contribution)
        .is_none_or(|when| (when.0)(world))
}

/// What fills the page of the contribution `contribution`, which
/// names the plugin panel `panel`.
pub fn content(
    world: &World,
    contribution: Entity,
    panel: &str,
) -> AnyView {
    match world.get::<Native>(contribution) {
        Some(Native(view)) => view(),
        None => tree::view::content(panel),
    }
}
