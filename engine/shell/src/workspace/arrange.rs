//! A workspace's layout put into the dock, and read back out of it.

use bevy_fynix::dock::{
    DockAreaStyle, DockLeaf, DockNode, DockSplit, DockTree, NodeId,
    SplitAxis,
};
use hamlet_engine_api::{Axis, Layout, PanelId};

/// The least share either side of a split keeps.
const LEAST_SHARE: f32 = 0.05;

/// Lays `layout` out in `tree`, leaving out the panels not `known`
/// and the areas left with none. The tree's ids go on from those it
/// gave before, so no view of the old layout is taken for the new.
pub fn lay_out(
    tree: &mut DockTree,
    layout: &Layout,
    known: impl Fn(&str) -> bool,
) {
    tree.nodes.clear();
    tree.root = None;
    let mut areas = 0;
    let root = build(tree, layout, &known, &mut areas);
    match root {
        Some(root) => tree.root = Some(root),
        None => {
            tree.set_root_leaf(DockLeaf::new(
                "area_0",
                DockAreaStyle::TabBar,
            ));
        }
    }
}

fn build(
    tree: &mut DockTree,
    layout: &Layout,
    known: &impl Fn(&str) -> bool,
    areas: &mut usize,
) -> Option<NodeId> {
    match layout {
        Layout::Tabs { panels, shown } => {
            let front = panels.get(*shown);
            let windows = panels
                .iter()
                .filter(|panel| known(panel.as_str()))
                .map(|panel| panel.as_str().to_owned())
                .collect::<Vec<_>>();
            if windows.is_empty() {
                return None;
            }
            let at = front
                .and_then(|front| {
                    windows
                        .iter()
                        .position(|window| window == front.as_str())
                })
                .unwrap_or(0);
            let leaf = DockLeaf::new(
                format!("area_{areas}"),
                DockAreaStyle::TabBar,
            )
            .with_windows(windows);
            *areas += 1;
            let id = tree.insert(DockNode::Leaf(leaf));
            if let Some(DockNode::Leaf(leaf)) = tree.get_mut(id) {
                leaf.active = leaf.windows.get(at).map(|tab| tab.id);
            }
            Some(id)
        }
        Layout::Split {
            axis,
            share,
            first,
            second,
        } => {
            let a = build(tree, first, known, areas);
            let b = build(tree, second, known, areas);
            match (a, b) {
                (Some(a), Some(b)) => {
                    Some(tree.insert(DockNode::Split(
                        DockSplit {
                            axis: match axis {
                                Axis::Horizontal => {
                                    SplitAxis::Horizontal
                                }
                                Axis::Vertical => SplitAxis::Vertical,
                            },
                            fraction: share.clamp(
                                LEAST_SHARE,
                                1.0 - LEAST_SHARE,
                            ),
                            a,
                            b,
                        },
                    )))
                }
                (one, other) => one.or(other),
            }
        }
    }
}

/// The layout `tree` shows.
pub fn layout_of(tree: &DockTree) -> Layout {
    tree.root
        .and_then(|root| read(tree, root))
        .unwrap_or_else(Layout::empty)
}

fn read(tree: &DockTree, id: NodeId) -> Option<Layout> {
    Some(match tree.get(id)? {
        DockNode::Leaf(leaf) => {
            let panels = leaf
                .windows
                .iter()
                .filter_map(|tab| {
                    PanelId::try_from(tab.window_id.as_str()).ok()
                })
                .collect();
            let shown = leaf
                .active
                .and_then(|active| leaf.tab_index(active))
                .unwrap_or(0);
            Layout::Tabs { panels, shown }
        }
        DockNode::Split(split) => Layout::Split {
            axis: match split.axis {
                SplitAxis::Horizontal => Axis::Horizontal,
                SplitAxis::Vertical => Axis::Vertical,
            },
            share: split.fraction,
            first: Box::new(read(tree, split.a)?),
            second: Box::new(read(tree, split.b)?),
        },
    })
}
