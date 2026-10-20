//! The dock and its panels. A panel is an entity, and the dock can
//! show it while the entity is there.

use bevy::app::App;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::lifecycle::{Add, Remove};
use bevy::ecs::observer::On;
use bevy::ecs::system::{Commands, Query, ResMut};
use bevy::ui::{Overflow, UiRect, px};
use bevy_fynix::dock::{
    DockPlugin, DockRegistry, DockTree, DockWindowKind,
};
use bevy_fynix::views::{FrameProps, column};
use hamlet_engine_api::id::ENGINE;
use hamlet_engine_api::{CommandId, PanelId};

use crate::command::{RunCommand, engine_command};
use crate::{AnyView, Cx, EngineTheme, View};

pub(crate) fn plugin(app: &mut App) {
    app.add_plugins(DockPlugin::<EngineTheme>::default())
        .add_observer(offer)
        .add_observer(withdraw);
}

/// A panel the dock can show.
#[derive(Component, Clone, Debug)]
#[component(immutable)]
pub struct Panel {
    pub id: PanelId,
    pub title: String,
    /// Builds its content, given the panel's entity.
    pub content: fn(Entity) -> AnyView,
}

/// A panel's content: `view`, inside the panel's padding, cut where
/// it overflows.
pub fn panel(view: impl View) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let theme = cx.theme();
        let (fill, inset) =
            (theme.colors.background, theme.space.panel);
        cx.build(
            column((view,))
                .grow(1.0)
                .min_width(px(0.0))
                .min_height(px(0.0))
                .padding(UiRect::all(px(inset)))
                .fill(fill)
                .overflow(Overflow::clip()),
        )
    })
}

/// Brings the panel `id` to the front, or opens it in the first area
/// when the shown workspace does not have it.
pub fn open(tree: &mut DockTree, id: &str) {
    if let Some(leaf) = tree.find_leaf_with_window(id) {
        let tab = tree.leaf(leaf).and_then(|leaf| {
            leaf.tabs()
                .find(|(window, _)| *window == id)
                .map(|(_, tab)| tab)
        });
        if let Some(tab) = tab {
            tree.set_active(leaf, tab);
        }
        return;
    }
    let first = tree.root.and_then(|root| {
        tree.leaves_under(root).first().map(|(leaf, _)| *leaf)
    });
    if let Some(leaf) = first {
        tree.add_tab(leaf, id);
    }
}

/// Lets the dock show a new panel, and adds a command that opens it
/// to the View menu.
fn offer(
    added: On<Add, Panel>,
    panels: Query<&Panel>,
    mut registry: ResMut<DockRegistry<EngineTheme>>,
    mut commands: Commands,
) {
    let entity = added.entity;
    let Ok(panel) = panels.get(entity) else {
        return;
    };
    let content = panel.content;
    registry.register(
        panel.id.as_str(),
        DockWindowKind::new(panel.title.clone(), move || {
            content(entity)
        }),
    );
    let id = panel.id.clone();
    // Past the longest name, a panel is opened from a tab bar's "+".
    let Ok(command) = CommandId::new(
        ENGINE,
        &format!("show.{}", id.as_str().replace('/', ".")),
    ) else {
        return;
    };
    engine_command(
        &mut commands,
        command,
        &panel.title,
        Some("View"),
        &[],
    )
    .insert(ChildOf(entity))
    .observe(
        move |_: On<RunCommand>, mut tree: ResMut<DockTree>| {
            open(&mut tree, id.as_str());
        },
    );
}

/// Takes a panel that is going out of the dock.
fn withdraw(
    removed: On<Remove, Panel>,
    panels: Query<&Panel>,
    mut registry: ResMut<DockRegistry<EngineTheme>>,
    mut tree: ResMut<DockTree>,
) {
    let Ok(panel) = panels.get(removed.entity) else {
        return;
    };
    registry.unregister(panel.id.as_str());
    tree.remove_window_kind(panel.id.as_str());
}
