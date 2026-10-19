//! The dock of the gallery: the panels around a picture of the world.

use bevy::ecs::system::Local;
use bevy::ecs::world::World;
use bevy::math::Vec2;
use bevy::ui::widget::{ImageNode, NodeImageMode};
use bevy::ui::{percent, px};
use bevy_fynix::AnyView;
use bevy_fynix::dock::{
    DockAreaStyle, DockLeaf, DockRegistry, DockTree, DockWindowKind,
    Edge, dock,
};
use bevy_fynix::views::{FrameProps, column, frame, overlay};
use hamlet_core::session::{Entry, Transcript};
use hamlet_sdk::dock::show_tab;
use hamlet_ui::feel::Lit;
use hamlet_ui::icons::Glyph;
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::{
    Cx, ICON, Me, View, chat, lit_panel, panel, toolbar,
};
use plugin_menus::menu;
use plugin_menus::world_menu::{Deed, question};
use plugin_status::status_bar;

use super::{Scene, Stand, Ui};

/// The picture of the world over the command line.
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
            column((
                column(())
                    .grow(1.0)
                    .min_height(px(0.0))
                    .width(percent(100.0))
                    .with(picture.unwrap_or_default()),
                chat::message_bar(),
            ))
            .grow(1.0)
            .min_width(px(0.0))
            .min_height(px(0.0))
            .gap(0.0),
        )
    })
}

pub(super) fn seed_dock(world: &mut World) {
    let mut registry =
        world.resource_mut::<DockRegistry<HamletTheme>>();
    registry.register(
        "world",
        DockWindowKind::new("WORLD", world_stand)
            .icon(Glyph::Globe.image(ICON)),
    );
    registry.register(
        "needs",
        DockWindowKind::new("NEEDS YOU", || {
            lit_panel(
                plugin_needs::screen(),
                Lit {
                    alarm: true,
                    flash: false,
                },
            )
        })
        .icon(Glyph::Bell.image(ICON)),
    );
    registry.register(
        "agent",
        DockWindowKind::new("INSPECTOR", || {
            lit_panel(
                plugin_inspector::screen(),
                Lit {
                    alarm: false,
                    flash: true,
                },
            )
        })
        .icon(Glyph::Lens.image(ICON)),
    );
    registry.register(
        "settings",
        DockWindowKind::new("SETTINGS", || {
            panel(plugin_settings::screen())
        })
        .icon(Glyph::Sliders.image(ICON)),
    );
    let scene = *world.resource::<Scene>();
    let mut tree = world.resource_mut::<DockTree>();
    let centre = tree.set_root_leaf(
        DockLeaf::new("centre", DockAreaStyle::TabBar)
            .with_windows(vec!["world".into()]),
    );
    if let Some((queue, _)) =
        tree.split(centre, Edge::Left, "needs".into())
        && let Some(parent) = tree.parent_of(queue)
    {
        tree.set_fraction(parent, 0.24);
    }
    if let Some((side, agent)) =
        tree.split(centre, Edge::Right, "agent".into())
    {
        tree.set_active(side, agent);
        if let Some(parent) = tree.parent_of(side) {
            tree.set_fraction(parent, 0.62);
        }
        if let Some(tab) = scene_tab(scene) {
            tree.add_tab(side, &tab);
        }
    }
}

/// The tab the scene shows beside the Inspector, if it has one.
fn scene_tab(scene: Scene) -> Option<String> {
    match scene {
        Scene::Settings
        | Scene::Charter
        | Scene::Plugins
        | Scene::PluginPermissions
        | Scene::PluginCapabilities
        | Scene::PluginAdds
        | Scene::PluginReload
        | Scene::PluginSettingsPage => Some("settings".into()),
        Scene::Skills | Scene::SkillsAlone => Some("skills".into()),
        Scene::PluginRoster => Some(super::roster::id()),
        Scene::PluginCanvas => Some(super::canvas::id()),
        Scene::PluginOpenui => Some(super::greeter::id()),
        Scene::PluginNotes | Scene::PluginNotesPreview => {
            Some(super::notes::id())
        }
        Scene::PluginClipper => Some(super::clipper::id()),
        _ => None,
    }
}

/// Brings the scene's tab forward again once, after the Inspector
/// has come to the front for the selection.
pub(super) fn front_scene_tab(
    world: &mut World,
    mut done: Local<bool>,
) {
    if *done {
        return;
    }
    *done = true;
    if let Some(tab) = scene_tab(*world.resource::<Scene>()) {
        show_tab(world, &tab);
    }
}

/// The question before a rewind to the last of the chief's
/// messages, in the scene that shows it.
fn rewind_question() -> Ui {
    AnyView::new(|cx: &mut Cx| {
        let session = cx.world.resource::<Me>().session;
        let last = cx
            .world
            .get::<Transcript>(session)
            .into_iter()
            .flat_map(|said| said.items().iter().rev())
            .find(|item| matches!(item.entry, Entry::User { .. }))
            .map(|item| item.id)
            .filter(|_| {
                *cx.world.resource::<Scene>() == Scene::Rewind
            });
        let Some(entry) = last else {
            return cx.build(frame());
        };
        let at = Vec2::new(1060.0, 526.0);
        let deed = Deed::Rewind { session, entry };
        cx.build(overlay((question(deed, at),)))
    })
}

/// The menu of a thing in the world, in the scenes that show it.
fn world_menu() -> Ui {
    AnyView::new(|cx: &mut Cx| match *cx.world.resource::<Scene>() {
        Scene::WorldMenu(_) | Scene::PluginContext => {
            cx.build(plugin_menus::world_menu::view())
        }
        _ => cx.build(frame()),
    })
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
        toolbar(cx),
        column((dock::<HamletTheme>(), plugin_welcome::view()))
            .grow(1.0)
            .min_height(px(0.0))
            .gap(0.0),
        status_bar(),
        menu::open_menu(),
        rewind_question(),
        world_menu(),
    ))
    .width(percent(100.0))
    .height(percent(100.0))
    .fill(cx.theme().background())
    .gap(0.0)
}
