//! The dock as a feature sees it: registering a panel, what fills
//! one and bringing one forward.

use bevy::app::App;
use bevy::ecs::event::Event;
use bevy::ecs::world::World;
use bevy::ui::{Overflow, UiRect, px};
use bevy_fynix::dock::{DockRegistry, DockTree, DockWindowKind};
use bevy_fynix::views::{BehaviorExt, FrameProps, column};

use crate::feel::Lit;
use crate::looks::Fills;
use crate::platform::{ClipInside, Glow};
use crate::theme::HamletTheme;
use crate::{AnyView, Cx, View};

/// Registers a panel's dock window from its own plugin.
pub trait PanelAppExt {
    fn add_panel(
        &mut self,
        id: &str,
        kind: DockWindowKind<HamletTheme>,
    ) -> &mut Self;
}

impl PanelAppExt for App {
    fn add_panel(
        &mut self,
        id: &str,
        kind: DockWindowKind<HamletTheme>,
    ) -> &mut Self {
        self.init_resource::<DockRegistry<HamletTheme>>();
        self.world_mut()
            .resource_mut::<DockRegistry<HamletTheme>>()
            .register(id, kind);
        self
    }
}

/// A dock panel's contents.
pub fn panel(view: impl View) -> AnyView {
    lit_panel(view, Lit::default())
}

/// A panel that light falls into as things happen in the hamlet.
pub fn lit_panel(view: impl View, lit: Lit) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let fill = cx.theme().palette.black;
        // The dock frames the area; this is what fills it.
        cx.build(
            column((view,))
                .fills_width()
                .min_height(px(0.0))
                .padding(UiRect::all(px(16.0)))
                .gap(0.0)
                .fill(fill)
                // Overflow is cut at the padding.
                .overflow(Overflow::clip())
                .tagged(ClipInside)
                .tagged(Glow)
                .tagged(lit),
        )
    })
}

/// The window kind `0` was brought to the front.
#[derive(Event)]
pub struct TabShown(pub String);

/// Brings a panel's tab to the front.
pub fn show_tab(world: &mut World, id: &str) {
    let mut tree = world.resource_mut::<DockTree>();
    let Some(leaf) = tree.find_leaf_with_window(id) else {
        return;
    };
    let tab = tree.leaf(leaf).and_then(|l| {
        l.tabs()
            .find(|(window, _)| *window == id)
            .map(|(_, tab)| tab)
    });
    if let Some(tab) = tab {
        tree.set_active(leaf, tab);
    }
    world.trigger(TabShown(id.to_string()));
}
