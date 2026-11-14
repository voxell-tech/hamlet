//! The dock of the gallery: the panels around a picture of the world.

use bevy::ecs::world::World;
use bevy::ui::widget::{ImageNode, NodeImageMode};
use bevy::ui::{percent, px};
use bevy_fynix::AnyView;
use bevy_fynix::dock::{
    DockAreaStyle, DockLeaf, DockRegistry, DockTree, DockWindowKind,
    Edge,
};
use bevy_fynix::views::{FrameProps, column};
use hamlet_app::{
    highlight_panel, hub, layout, menu, panel, settings, terminal,
};
use hamlet_ui::styles::ICON;
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::widgets::effects::Highlight;
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::{Cx, View};

use super::{Scene, Stand, Ui};

/// The picture of the world.
fn world_stand() -> Ui {
    AnyView::new(|cx: &mut Cx| {
        let picture =
            cx.world.get_resource::<Stand>().map(|stand| ImageNode {
                image: stand.0.clone(),
                rect: Some(stand.1),
                image_mode: NodeImageMode::Stretch,
                ..ImageNode::default()
            });
        cx.build(
            column(())
                .grow(1.0)
                .min_width(px(0.0))
                .min_height(px(0.0))
                .width(percent(100.0))
                .with(picture.unwrap_or_default()),
        )
    })
}

pub(super) fn seed_dock(world: &mut World) {
    let scene = *world.resource::<Scene>();
    let mut registry =
        world.resource_mut::<DockRegistry<HamletTheme>>();
    registry.register(
        "world",
        DockWindowKind::new("Realm", world_stand)
            .icon(Glyph::Globe.image(ICON)),
    );
    registry.register(
        "hub",
        DockWindowKind::new("Hub", || {
            highlight_panel(
                hub::screen(),
                Highlight {
                    attention: true,
                    flash: true,
                },
            )
        })
        .icon(Glyph::Tree.image(ICON)),
    );
    registry.register(
        "settings",
        DockWindowKind::new("Settings", || panel(settings::screen()))
            .icon(Glyph::Sliders.image(ICON)),
    );
    let mut tree = world.resource_mut::<DockTree>();
    let centre = tree.set_root_leaf(
        DockLeaf::new("centre", DockAreaStyle::TabBar)
            .with_windows(vec!["world".into()]),
    );
    if let Some((side, _)) =
        tree.split(centre, Edge::Right, "hub".into())
        && let Some(parent) = tree.parent_of(side)
    {
        tree.set_fraction(parent, 0.5);
    }
    if matches!(
        scene,
        Scene::Terminal
            | Scene::TerminalShell
            | Scene::TerminalPanels
            | Scene::TerminalPick
    ) && let Some((low, _)) =
        tree.split(centre, Edge::Bottom, "terminal".into())
        && let Some(parent) = tree.parent_of(low)
    {
        tree.set_fraction(parent, 0.4);
        if scene == Scene::TerminalPanels {
            tree.split(low, Edge::Right, "terminal-2".into());
        }
    }
    hamlet_app::register_numbered(world);
}

/// Opens the Settings tab beside the Hub in the scenes of Settings.
/// After the first update, as the desk lays its own dock out over the
/// seeded one then.
pub(super) fn open_settings(
    world: &mut World,
    mut opened: bevy::ecs::system::Local<bool>,
) {
    let scene = *world.resource::<Scene>();
    if *opened
        || !matches!(
            scene,
            Scene::Settings
                | Scene::SettingsModels
                | Scene::Charter
                | Scene::Workspace
                | Scene::Sight
                | Scene::Appearance(_)
        )
    {
        return;
    }
    *opened = true;
    let mut tree = world.resource_mut::<DockTree>();
    if let Some(side) = tree.find_leaf_with_window("hub") {
        tree.add_tab(side, "settings");
    }
}

pub(super) fn panels() -> Ui {
    AnyView::new(|cx: &mut Cx| {
        let panels = shown(cx);
        cx.build(panels)
    })
}

/// The window's panels as the app lays them out.
fn shown(cx: &Cx) -> impl View {
    column((
        layout(cx),
        hamlet_app::toasts::stack(),
        menu::open_menu(),
        terminal::open_menu(),
    ))
    .width(percent(100.0))
    .height(percent(100.0))
    .min_height(px(0.0))
    .fill(cx.theme().background())
    .gap(0.0)
}
