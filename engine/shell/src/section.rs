//! Sections plugins add to an engine panel that takes them.

use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy_fynix::views::column;
use hamlet_engine_api::PanelId;

use crate::{AnyView, Cx};

/// A section a plugin adds to the engine's panel `panel`.
#[derive(Component, Clone, Debug)]
pub struct PanelSection {
    pub id: PanelId,
    pub panel: PanelId,
    pub title: String,
    /// How many things it holds, as its plugin says. It is shown
    /// while it holds any.
    pub holds: u32,
    /// Builds its content, given the section's entity.
    pub content: fn(Entity) -> AnyView,
}

/// The content of the section `at`, as its plugin builds it.
pub fn section_content(at: Entity) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        match cx.world.get::<PanelSection>(at).map(|it| it.content) {
            Some(content) => cx.build(content(at)),
            None => cx.build(column(())),
        }
    })
}
