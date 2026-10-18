//! A panel of the dock, as an entity.

use bevy::app::App;
use bevy::ecs::component::Component;
use bevy::ecs::lifecycle::{Add, Remove};
use bevy::ecs::observer::On;
use bevy::ecs::system::{Query, ResMut};
use bevy_fynix::dock::{DockRegistry, DockTree, DockWindowKind};

use crate::AnyView;
use crate::icons::Glyph;
use crate::looks::ICON;
use crate::theme::HamletTheme;

/// A window of the dock, which is in the dock's registry for as long
/// as this entity is.
#[derive(Component, Clone)]
#[component(immutable)]
pub struct Panel {
    pub id: String,
    pub title: String,
    pub icon: Glyph,
    /// Builds the panel's contents.
    pub view: fn() -> AnyView,
}

impl Panel {
    pub fn new(
        id: impl Into<String>,
        title: impl Into<String>,
        icon: Glyph,
        view: fn() -> AnyView,
    ) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            icon,
            view,
        }
    }
}

/// Adds a panel that no plugin owns. [`crate::SdkPlugin`] is added
/// first.
pub trait PanelAppExt {
    fn add_panel(&mut self, panel: Panel) -> &mut Self;
}

impl PanelAppExt for App {
    fn add_panel(&mut self, panel: Panel) -> &mut Self {
        self.world_mut().spawn(panel);
        self
    }
}

pub(super) fn register(
    add: On<Add, Panel>,
    panels: Query<&Panel>,
    mut registry: ResMut<DockRegistry<HamletTheme>>,
) {
    let Ok(panel) = panels.get(add.entity) else {
        return;
    };
    registry.register(
        panel.id.clone(),
        DockWindowKind::new(panel.title.clone(), panel.view)
            .icon(panel.icon.image(ICON)),
    );
}

/// Takes the panel's window out of the registry and out of the dock.
pub(super) fn unregister(
    remove: On<Remove, Panel>,
    panels: Query<&Panel>,
    mut registry: ResMut<DockRegistry<HamletTheme>>,
    tree: Option<ResMut<DockTree>>,
) {
    let Ok(panel) = panels.get(remove.entity) else {
        return;
    };
    registry.unregister(&panel.id);
    if let Some(mut tree) = tree {
        tree.remove_window_kind(&panel.id);
    }
}
