//! The panels of the dock: adding one, the steps they take each frame
//! before fynix mounts them, and bringing one forward.

use std::collections::HashSet;

use bevy::app::{App, PreUpdate};
use bevy::ecs::event::Event;
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::{IntoScheduleConfigs, SystemSet};
use bevy::ecs::system::ResMut;
use bevy::ecs::world::World;
use bevy::input::InputSystems;
use bevy::picking::PickingSystems;
use bevy::ui::{Overflow, UiRect, px};
use bevy_fynix::dock::{
    DockRegistry, DockTree, DockWindowKind, DropTarget,
};
use bevy_fynix::views::{BehaviorExt, FrameProps, column};
use hamlet_client::connection::ClientSystems;
use hamlet_ui::styles::Fills;
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::widgets::effects::{
    GlowPanel, Highlight, glow_layer, highlight_layer,
};
use hamlet_ui::widgets::platform::{ClipInside, Glow};
use hamlet_ui::{AnyView, Cx, View};

pub fn plugin(app: &mut App) {
    app.configure_sets(
        // fynix mounts in `Update`, in sets it keeps private.
        PreUpdate,
        (PanelSystems::Follow, PanelSystems::Bind)
            .chain()
            .after(ClientSystems::Receive)
            .after(InputSystems)
            .after(PickingSystems::Last),
    )
    .init_resource::<Numbered>()
    .init_resource::<OnDesks>()
    .add_observer(show);
}

/// The panels' steps before fynix mounts the views.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum PanelSystems {
    /// The panels' state follows what was received or clicked.
    Follow,
    /// The values the views read, worked out from Follow's.
    Bind,
}

/// Registers a panel's dock window from its own plugin.
pub trait PanelAppExt {
    fn add_panel(
        &mut self,
        id: &str,
        kind: DockWindowKind<HamletTheme>,
    ) -> &mut Self;

    /// Registers a panel that opens more than once: `id` now, and
    /// each other as it is opened or laid out.
    fn add_numbered_panel(
        &mut self,
        id: &str,
        make: MakePanel,
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

    fn add_numbered_panel(
        &mut self,
        id: &str,
        make: MakePanel,
    ) -> &mut Self {
        self.add_panel(id, make(id));
        self.init_resource::<Numbered>();
        self.world_mut()
            .resource_mut::<Numbered>()
            .0
            .push((id.to_owned(), make));
        self
    }
}

/// What makes the dock window of a numbered panel from its id.
pub type MakePanel = fn(&str) -> DockWindowKind<HamletTheme>;

/// The panels that open more than once, by the id of the first, with
/// what makes a window of each. The others are `<id>-2`, `<id>-3` and
/// so on, registered as they are opened or laid out.
#[derive(Resource, Default)]
pub struct Numbered(Vec<(String, MakePanel)>);

impl Numbered {
    /// The ids of the first of each numbered panel.
    pub fn firsts(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(|(first, _)| first.as_str())
    }
}

/// The ids of the panels on every desk, shown or not.
#[derive(Resource, Default, PartialEq)]
pub struct OnDesks(pub HashSet<String>);

/// The number of `id` as another of the numbered panel `first`.
pub fn number(first: &str, id: &str) -> Option<u32> {
    id.strip_prefix(first)?
        .strip_prefix('-')?
        .parse()
        .ok()
        .filter(|n| *n >= 2)
}

/// Whether a desk holds the panel `id`.
pub fn on_a_desk(world: &World, id: &str) -> bool {
    world
        .get_resource::<OnDesks>()
        .is_some_and(|desks| desks.0.contains(id))
}

/// The id of a copy of the numbered panel `first` that neither the
/// dock nor a desk holds.
pub fn free_copy(world: &World, first: &str) -> String {
    (2u32..)
        .map(|n| format!("{first}-{n}"))
        .find(|id| {
            world
                .resource::<DockTree>()
                .find_leaf_with_window(id)
                .is_none()
                && !on_a_desk(world, id)
        })
        .unwrap_or_default()
}

/// Registers the numbered panel `id`, unless it is or it is not one.
pub fn register(world: &mut World, id: &str) {
    let Some(make) = world.get_resource::<Numbered>().and_then(|n| {
        n.0.iter()
            .find(|(first, _)| number(first, id).is_some())
            .map(|(_, make)| *make)
    }) else {
        return;
    };
    let mut registry =
        world.resource_mut::<DockRegistry<HamletTheme>>();
    if registry.get(id).is_none() {
        registry.register(id, make(id));
    }
}

/// Opens another of the numbered panel `first` where `target` says,
/// and gives its id: one neither the dock nor a desk holds.
pub fn open_numbered(
    world: &mut World,
    first: &str,
    target: DropTarget,
) -> Option<String> {
    let id = free_copy(world, first);
    register(world, &id);
    let mut tree = world.resource_mut::<DockTree>();
    let placed = match target {
        DropTarget::Tabs { leaf, index } => {
            tree.add_tab(leaf, id.clone()).map(|tab| {
                tree.insert_tab(tab, leaf, true, Some(index));
            })
        }
        DropTarget::Area(leaf) => {
            tree.add_tab(leaf, id.clone()).map(|_| ())
        }
        DropTarget::Edge { leaf, edge } => {
            tree.split(leaf, edge, id.clone()).map(|_| ())
        }
    };
    if placed.is_none() {
        world
            .resource_mut::<DockRegistry<HamletTheme>>()
            .unregister(&id);
        return None;
    }
    Some(id)
}

/// A dock panel's contents.
pub fn panel(view: impl View) -> AnyView {
    highlight_panel(view, Highlight::default())
}

/// A panel that is highlighted as things happen in the hamlet.
pub fn highlight_panel(
    view: impl View,
    highlight: Highlight,
) -> AnyView {
    padded_panel(view, highlight, None)
}

/// A panel whose contents reach its edges, for a canvas or a picture
/// that draws its own margins.
pub fn flush_panel(view: impl View) -> AnyView {
    padded_panel(view, Highlight::default(), Some(0.0))
}

/// A panel padded by `padding`, else by the theme's panel space.
fn padded_panel(
    view: impl View,
    highlight: Highlight,
    padding: Option<f32>,
) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let fill = cx.theme().palette.surface;
        let padding = padding.unwrap_or(cx.theme().space.panel);
        // The dock frames the area; this is what fills it.
        cx.build(
            column((highlight_layer(highlight), glow_layer(), view))
                .fills_width()
                .min_height(px(0.0))
                .padding(UiRect::all(px(padding)))
                .gap(0.0)
                .fill(fill)
                // Overflow is cut at the padding.
                .overflow(Overflow::clip())
                .tagged(ClipInside)
                .tagged(Glow)
                .tagged(GlowPanel),
        )
    })
}

/// Whether the panel `id`'s tab is in front in the desk shown.
pub fn is_in_front(tree: &DockTree, id: &str) -> bool {
    let Some(leaf) = tree.find_leaf_with_window(id) else {
        return false;
    };
    let front = tree.active(leaf);
    tree.leaf(leaf).is_some_and(|l| {
        l.tabs()
            .any(|(window, tab)| window == id && Some(tab) == front)
    })
}

/// Brings the panel of this id's tab to the front, or opens it in the
/// first area when the desk shown does not have it.
#[derive(Event, Clone, Debug, PartialEq)]
pub struct ShowPanel(pub String);

fn show(show: On<ShowPanel>, mut tree: ResMut<DockTree>) {
    let id = show.0.as_str();
    let Some(leaf) = tree.find_leaf_with_window(id) else {
        let first = tree.root.and_then(|root| {
            tree.leaves_under(root).first().map(|(leaf, _)| *leaf)
        });
        if let Some(leaf) = first {
            tree.add_tab(leaf, id);
        }
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
}
