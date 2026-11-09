//! The dock: the panels there are, where each starts, and bringing
//! one forward.

use std::collections::HashMap;

use bevy::app::{App, PreUpdate, Update};
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::schedule::common_conditions::resource_changed;
use bevy::ecs::system::{Query, ResMut};
use bevy::ecs::world::World;
use bevy::ui::{ComputedNode, Overflow, UiRect, px};
use bevy_fynix::dock::{
    DockArea, DockAreaStyle, DockLeaf, DockNode, DockRegistry,
    DockTree, DockWindowKind, DropTarget, Edge, NodeId, SplitAxis,
};
use bevy_fynix::views::{BehaviorExt, FrameProps, column};
use hamlet_client::waits::Waiting;
use hamlet_ui::styles::Fills;
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::widgets::effects::{
    Attention, GlowPanel, Highlight, glow_layer, highlight_layer,
};
use hamlet_ui::widgets::platform::{ClipInside, Glow};
use hamlet_ui::{AnyView, Cx, View};

use crate::desk::{Desks, Part};
use crate::{PanelSystems, desk, remember};

/// The least width a panel keeps, in logical pixels.
const LEAST: f32 = 300.0;

/// The share of the window's width the realm takes on a new desk.
pub(crate) const REALM_SHARE: f32 = 0.5;

/// The dock window of the Hub.
pub(crate) const HUB: &str = "hub";

/// The ids of panels a stored dock may still name that the window no
/// longer has.
const GONE: [&str; 2] = ["agent", "needs"];

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Numbered>()
        .add_systems(
            PreUpdate,
            count_waits.in_set(PanelSystems::Bind),
        )
        .add_systems(
            Update,
            (
                keep_wide,
                register_numbered
                    .run_if(resource_changed::<Desks>)
                    .before(desk::show),
                prune_numbered
                    .run_if(resource_changed::<DockTree>)
                    .after(desk::capture),
            ),
        );
}

/// Tells the panels' highlights how many things wait for the chief.
fn count_waits(
    waiting: Query<(), With<Waiting>>,
    mut attention: ResMut<Attention>,
) {
    let count = waiting.iter().count();
    if attention.0 != count {
        attention.0 = count;
    }
}

/// Registers a panel's dock window from its own plugin.
pub(crate) trait PanelAppExt {
    fn add_panel(
        &mut self,
        id: &str,
        kind: DockWindowKind<HamletTheme>,
    ) -> &mut Self;

    /// Registers a panel that opens more than once: `id` now, and
    /// each other as it is opened or laid out.
    #[cfg_attr(
        not(any(test, all(feature = "web", target_os = "macos"))),
        expect(
            dead_code,
            reason = "only the Web panels open more than once"
        )
    )]
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
pub(crate) type MakePanel = fn(&str) -> DockWindowKind<HamletTheme>;

/// The panels that open more than once, by the id of the first, with
/// what makes a window of each. The others are `<id>-2`, `<id>-3` and
/// so on, registered as they are opened or laid out.
#[derive(Resource, Default)]
pub(crate) struct Numbered(Vec<(String, MakePanel)>);

/// The number of `id` as another of the numbered panel `first`.
pub(crate) fn number(first: &str, id: &str) -> Option<u32> {
    id.strip_prefix(first)?
        .strip_prefix('-')?
        .parse()
        .ok()
        .filter(|n| *n >= 2)
}

/// The ids of the panels in the tabs of `parts`.
fn tabs_of(parts: &[Part]) -> impl Iterator<Item = &String> {
    parts.iter().flat_map(|part| match part {
        Part::Tabs { tabs, .. } => tabs.as_slice(),
        Part::Split { .. } => &[],
    })
}

/// Whether a desk holds the panel `id`.
pub(crate) fn on_a_desk(world: &World, id: &str) -> bool {
    world.get_resource::<Desks>().is_some_and(|desks| {
        desks
            .list()
            .iter()
            .any(|desk| tabs_of(&desk.dock).any(|tab| tab == id))
    })
}

/// Registers the numbered panels the shown desk and the dock hold, so
/// the desk is laid out with them.
pub fn register_numbered(world: &mut World) {
    let mut ids = world
        .get_resource::<Desks>()
        .and_then(|desks| desks.list().get(desks.shown()))
        .map(|desk| tabs_of(&desk.dock).cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    if let Some(tree) = world.get_resource::<DockTree>() {
        ids.extend(
            tree.nodes
                .values()
                .filter_map(DockNode::as_leaf)
                .flat_map(|leaf| {
                    leaf.tabs().map(|(id, _)| id.to_owned())
                }),
        );
    }
    for id in ids {
        register(world, &id);
    }
}

/// Forgets the numbered panels the dock no longer holds, so the
/// dock's add menu offers only the first of each.
fn prune_numbered(world: &mut World) {
    let firsts = world
        .resource::<Numbered>()
        .0
        .iter()
        .map(|(first, _)| first.clone())
        .collect::<Vec<_>>();
    let gone = {
        let tree = world.resource::<DockTree>();
        world
            .resource::<DockRegistry<HamletTheme>>()
            .iter()
            .map(|(id, _)| id)
            .filter(|id| {
                firsts.iter().any(|first| number(first, id).is_some())
            })
            .filter(|id| tree.find_leaf_with_window(id).is_none())
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    let mut registry =
        world.resource_mut::<DockRegistry<HamletTheme>>();
    for id in gone {
        registry.unregister(&id);
    }
}

/// Registers the numbered panel `id`, unless it is or it is not one.
fn register(world: &mut World, id: &str) {
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
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "no panel opens more than once yet")
)]
pub(crate) fn open_numbered(
    world: &mut World,
    first: &str,
    target: DropTarget,
) -> Option<String> {
    let id = (2u32..).map(|n| format!("{first}-{n}")).find(|id| {
        world
            .resource::<DockTree>()
            .find_leaf_with_window(id)
            .is_none()
            && !on_a_desk(world, id)
    })?;
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

/// Opens another of the numbered panel `first` beside the panel
/// `beside`, or beside the first area when it is not in the dock,
/// and gives its id.
#[expect(dead_code, reason = "no panel opens more than once yet")]
pub(crate) fn open_another(
    world: &mut World,
    first: &str,
    beside: &str,
) -> Option<String> {
    let leaf = {
        let tree = world.resource::<DockTree>();
        tree.find_leaf_with_window(beside).or_else(|| {
            let root = tree.root?;
            tree.leaves_under(root).first().map(|(leaf, _)| *leaf)
        })?
    };
    open_numbered(
        world,
        first,
        DropTarget::Edge {
            leaf,
            edge: Edge::Right,
        },
    )
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
#[cfg_attr(
    not(any(test, all(feature = "web", target_os = "macos"))),
    expect(dead_code, reason = "only the Web panels are flush")
)]
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

/// The width `node` needs for every panel in it to keep [`LEAST`],
/// and the width it is drawn at, from the width of each area.
fn widths(
    tree: &DockTree,
    drawn: &HashMap<NodeId, f32>,
    node: NodeId,
) -> (f32, f32) {
    match tree.get(node) {
        Some(DockNode::Split(split)) => {
            let (a, b) = (
                widths(tree, drawn, split.a),
                widths(tree, drawn, split.b),
            );
            match split.axis {
                SplitAxis::Horizontal => (a.0 + b.0, a.1 + b.1),
                SplitAxis::Vertical => (a.0.max(b.0), a.1.max(b.1)),
            }
        }
        _ => (LEAST, drawn.get(&node).copied().unwrap_or_default()),
    }
}

/// The share `a` has of a split `span` wide, moved no further than
/// leaves each side the `least` it needs. A span too narrow for
/// both is shared by what each needs.
fn held(share: f32, least: (f32, f32), span: f32) -> f32 {
    if least.0 + least.1 >= span {
        return least.0 / (least.0 + least.1);
    }
    share.clamp(least.0 / span, 1.0 - least.1 / span)
}

/// Keeps every panel at least [`LEAST`] wide, however a divider is
/// dragged and whatever size the window takes.
pub(crate) fn keep_wide(
    mut tree: ResMut<DockTree>,
    areas: Query<(&DockArea, &ComputedNode)>,
) {
    let drawn = areas
        .iter()
        .map(|(area, node)| {
            (area.leaf, node.size().x * node.inverse_scale_factor())
        })
        .collect::<HashMap<_, _>>();
    let moved = tree
        .nodes
        .iter()
        .filter_map(|(id, node)| {
            let split = node.as_split()?;
            if split.axis != SplitAxis::Horizontal {
                return None;
            }
            let (a, b) = (
                widths(&tree, &drawn, split.a),
                widths(&tree, &drawn, split.b),
            );
            let span = a.1 + b.1;
            if span <= 0.0 {
                return None;
            }
            let share = held(split.fraction, (a.0, b.0), span);
            ((share - split.fraction).abs() > 0.002)
                .then_some((*id, share))
        })
        .collect::<Vec<_>>();
    for (split, share) in moved {
        tree.set_fraction(split, share);
    }
}

/// Puts back the kept view and lays out the desk shown.
pub(crate) fn seed(world: &mut World) {
    remember::restore(world);
    register_numbered(world);
    // Here rather than in the next frame, so the panels are mounted
    // on the desk.
    let _ = world.run_system_cached(desk::show);
}

/// The dock the window starts with: the realm on the left, and the
/// Hub on the right.
pub(crate) fn designed(tree: &mut DockTree) {
    let centre = tree.set_root_leaf(
        DockLeaf::new("centre", DockAreaStyle::TabBar)
            .with_windows(vec!["world".into()]),
    );
    if let Some((side, _)) =
        tree.split(centre, Edge::Right, HUB.into())
        && let Some(parent) = tree.parent_of(side)
    {
        tree.set_fraction(parent, REALM_SHARE);
    }
}

/// Whether a stored dock names a panel the window no longer has.
pub(crate) fn is_stale(
    tabs: impl IntoIterator<Item = String>,
) -> bool {
    tabs.into_iter().any(|tab| GONE.contains(&tab.as_str()))
}

/// Brings a panel's tab to the front, or opens it in the first area
/// when the desk shown does not have it.
pub(crate) fn show_tab(world: &mut World, id: &str) {
    let mut tree = world.resource_mut::<DockTree>();
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

#[cfg(test)]
mod tests {
    use bevy::app::App;
    use bevy_fynix::dock::{DockAreaStyle, DockLeaf};
    use bevy_fynix::views::frame;

    use super::*;

    fn window(id: &str) -> DockWindowKind<HamletTheme> {
        DockWindowKind::new(id.to_owned(), || flush_panel(frame()))
    }

    #[test]
    fn a_numbered_panel_is_known_only_while_the_dock_or_the_shown_desk_holds_it()
     {
        let mut app = App::new();
        app.init_resource::<DockTree>()
            .add_numbered_panel("terminal", window);
        let desk = |tabs: &[&str]| {
            vec![Part::Tabs {
                area: "low".into(),
                tabs: tabs
                    .iter()
                    .map(|tab| (*tab).to_owned())
                    .collect(),
                front: 0,
                headless: false,
                kept: false,
            }]
        };
        let mut desks = serde_json::to_value(Desks::first(desk(&[
            "terminal",
            "terminal-3",
        ])))
        .unwrap();
        desks["list"].as_array_mut().unwrap().push(
            serde_json::json!({
                "name": "Other",
                "dock": desk(&["terminal-5"]),
            }),
        );
        app.insert_resource(
            serde_json::from_value::<Desks>(desks).unwrap(),
        );
        register_numbered(app.world_mut());
        let known = |app: &App, id: &str| {
            app.world()
                .resource::<DockRegistry<HamletTheme>>()
                .get(id)
                .is_some()
        };
        assert!(known(&app, "terminal-3"));
        // Another desk's is registered once that desk is shown.
        assert!(!known(&app, "terminal-5"));
        app.world_mut().resource_mut::<DockTree>().set_root_leaf(
            DockLeaf::new("low", DockAreaStyle::TabBar)
                .with_windows(vec!["terminal".into()]),
        );
        let leaf = app.world().resource::<DockTree>().root.unwrap();
        // The next number is one no desk holds.
        let opened = open_numbered(
            app.world_mut(),
            "terminal",
            DropTarget::Edge {
                leaf,
                edge: Edge::Right,
            },
        );
        assert_eq!(opened.as_deref(), Some("terminal-2"));
        let opened = open_numbered(
            app.world_mut(),
            "terminal",
            DropTarget::Area(leaf),
        );
        assert_eq!(opened.as_deref(), Some("terminal-4"));
        prune_numbered(app.world_mut());
        assert!(
            known(&app, "terminal-2") && known(&app, "terminal-4")
        );
        // Out of the dock, a panel leaves the add menu; the first
        // stays.
        assert!(!known(&app, "terminal-3"));
        assert!(known(&app, "terminal"));
    }

    #[test]
    fn a_desk_that_names_an_old_panel_starts_again() {
        assert!(is_stale(["world".into(), "agent".into()]));
        assert!(is_stale([HUB.into(), "needs".into()]));
        assert!(!is_stale(["world".into(), HUB.into()]));
    }
}
