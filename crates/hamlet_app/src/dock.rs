//! The dock: the panels there are, where each starts, and bringing
//! one forward.

use std::collections::HashMap;

use bevy::app::{App, PreUpdate, Update};
use bevy::ecs::query::With;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Query, ResMut};
use bevy::ecs::world::World;
use bevy::ui::{ComputedNode, Overflow, UiRect, px};
use bevy_fynix::dock::{
    DockArea, DockAreaStyle, DockLeaf, DockNode, DockRegistry,
    DockTree, DockWindowKind, Edge, NodeId, SplitAxis,
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

use crate::{PanelSystems, desk, remember};

/// The least width a panel keeps, in logical pixels.
const LEAST: f32 = 300.0;

/// The share of the window's width the Needs You queue takes.
pub(crate) const NEEDS_SHARE: f32 = 0.2;

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(
        PreUpdate,
        count_waits.in_set(PanelSystems::Bind),
    )
    .add_systems(Update, keep_wide);
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
    highlight_panel(view, Highlight::default())
}

/// A panel that is highlighted as things happen in the hamlet.
pub fn highlight_panel(
    view: impl View,
    highlight: Highlight,
) -> AnyView {
    padded_panel(view, highlight, 16.0)
}

/// A panel whose contents reach its edges, for a canvas or a picture
/// that draws its own margins.
pub fn flush_panel(view: impl View) -> AnyView {
    padded_panel(view, Highlight::default(), 0.0)
}

fn padded_panel(
    view: impl View,
    highlight: Highlight,
    padding: f32,
) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let fill = cx.theme().palette.surface;
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
    // Here rather than in the next frame, so the panels are mounted
    // on the desk.
    let _ = world.run_system_cached(desk::show);
}

/// The dock the window starts with: the world in the centre, and the
/// working panels as tabs beside it.
pub(crate) fn designed(tree: &mut DockTree) {
    let centre = tree.set_root_leaf(
        DockLeaf::new("centre", DockAreaStyle::TabBar)
            .with_windows(vec!["world".into()]),
    );
    // The queue has the left edge to itself.
    if let Some((queue, _)) =
        tree.split(centre, Edge::Left, "needs".into())
        && let Some(parent) = tree.parent_of(queue)
    {
        tree.set_fraction(parent, NEEDS_SHARE);
    }
    if let Some((side, agent)) =
        tree.split(centre, Edge::Right, "agent".into())
    {
        tree.set_active(side, agent);
        if let Some(parent) = tree.parent_of(side) {
            tree.set_fraction(parent, 0.64);
        }
    }
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
