//! The dock: the panels there are, where each starts, and bringing
//! one forward.

use std::collections::HashMap;

use bevy::app::{App, Update};
use bevy::ecs::observer::On;
use bevy::ecs::system::{Commands, Query, ResMut};
use bevy::ecs::world::World;
use bevy::ui::ComputedNode;
use bevy_fynix::dock::{
    DockArea, DockAreaStyle, DockLeaf, DockNode, DockTree, Edge,
    NodeId, SplitAxis,
};
use hamlet_core::core::RefreshLists;
use hamlet_sdk::dock::TabShown;

use crate::remember;

/// The least width a panel keeps, in logical pixels.
const LEAST: f32 = 300.0;

/// The share of the window's width the Needs You queue takes.
pub(crate) const NEEDS_SHARE: f32 = 0.2;

pub(crate) fn plugin(app: &mut App) {
    app.add_observer(refresh_on_agent)
        .add_systems(Update, keep_wide);
}

/// The Inspector coming to the front reads the lists again.
fn refresh_on_agent(shown: On<TabShown>, mut commands: Commands) {
    if shown.0 == "agent" {
        commands.trigger(RefreshLists);
    }
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

/// The world in the centre, and the working panels as tabs beside it.
pub(crate) fn seed(world: &mut World) {
    if remember::restore(world) {
        return;
    }
    let mut tree = world.resource_mut::<DockTree>();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_divider_stops_where_a_panel_would_be_too_narrow() {
        let least = (LEAST, 2.0 * LEAST);
        // Room for all: the share is kept, or moved to the nearest
        // that leaves each side what it needs.
        assert_eq!(held(0.5, least, 1200.0), 0.5);
        assert_eq!(held(0.1, least, 1200.0), 0.25);
        assert_eq!(held(0.9, least, 1200.0), 0.5);
        // Too narrow for all: each side by what it needs.
        assert_eq!(held(0.9, least, 600.0), 1.0 / 3.0);
    }
}
