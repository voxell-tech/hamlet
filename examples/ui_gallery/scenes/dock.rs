//! The dock of the gallery: the panels around a picture of the world.

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
use hamlet_ui::feel::Lit;
use hamlet_ui::icons::Glyph;
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::{
    Cx, ICON, Me, View, chat, inspector, lit_panel, menu, panel,
    petition, settings, status_bar, toolbar, welcome,
};

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
                petition::screen(),
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
                inspector::screen(),
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
        DockWindowKind::new("SETTINGS", || panel(settings::screen()))
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
        if matches!(
            scene,
            Scene::Settings
                | Scene::Charter
                | Scene::Plugins
                | Scene::PluginPermissions
                | Scene::PluginCapabilities
                | Scene::PluginReload
        ) {
            tree.add_tab(side, "settings");
        }
        if matches!(scene, Scene::Skills | Scene::SkillsAlone) {
            tree.add_tab(side, "skills");
        }
        if scene == Scene::PluginRoster {
            tree.add_tab(side, super::roster::id());
        }
        if scene == Scene::PluginOpenui {
            tree.add_tab(side, super::greeter::id());
        }
        if matches!(
            scene,
            Scene::PluginNotes | Scene::PluginNotesPreview
        ) {
            tree.add_tab(side, super::notes::id());
        }
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
        cx.build(overlay((chat::rewind_asked(session, entry, at),)))
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
        column((dock::<HamletTheme>(), welcome::view()))
            .grow(1.0)
            .min_height(px(0.0))
            .gap(0.0),
        status_bar(),
        menu::open_menu(),
        rewind_question(),
    ))
    .width(percent(100.0))
    .height(percent(100.0))
    .fill(cx.theme().background())
    .gap(0.0)
}
