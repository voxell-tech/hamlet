//! The dock's tree: a panel brought to the front, and a workspace's
//! layout put into it and read back out of it.

use bevy_fynix::dock::{
    DockAreaStyle, DockLeaf, DockNode, DockSplit, DockTree, NodeId,
    SplitAxis,
};
use hamlet_engine_api::{Axis, Layout, PanelId};

/// The least share either side of a split keeps.
const LEAST_SHARE: f32 = 0.05;

/// The shell's methods on [`DockTree`].
pub trait DockTreeExt {
    /// Brings the panel `id` to the front, or opens it in the first
    /// area when the shown workspace does not have it.
    fn open_panel(&mut self, id: &str);

    /// Lays `layout` out, leaving out the panels not `known` and the
    /// areas left with none. The tree's ids go on from those it gave
    /// before, so no view of the old layout is taken for the new.
    fn lay_out(
        &mut self,
        layout: &Layout,
        known: impl Fn(&str) -> bool,
    );

    /// The layout the tree shows.
    fn layout(&self) -> Layout;

    /// The layout under the node `id`.
    fn layout_at(&self, id: NodeId) -> Option<Layout>;
}

impl DockTreeExt for DockTree {
    fn open_panel(&mut self, id: &str) {
        if let Some(leaf) = self.find_leaf_with_window(id) {
            let tab = self.leaf(leaf).and_then(|leaf| {
                leaf.tabs()
                    .find(|(window, _)| *window == id)
                    .map(|(_, tab)| tab)
            });
            if let Some(tab) = tab {
                self.set_active(leaf, tab);
            }
            return;
        }
        let first = self.root.and_then(|root| {
            self.leaves_under(root).first().map(|(leaf, _)| *leaf)
        });
        if let Some(leaf) = first {
            self.add_tab(leaf, id);
        }
    }

    fn lay_out(
        &mut self,
        layout: &Layout,
        known: impl Fn(&str) -> bool,
    ) {
        self.nodes.clear();
        self.root = None;
        let root = Arranging {
            tree: self,
            known: &known,
            areas: 0,
        }
        .build(layout);
        match root {
            Some(root) => self.root = Some(root),
            None => {
                self.set_root_leaf(DockLeaf::new(
                    "area_0",
                    DockAreaStyle::TabBar,
                ));
            }
        }
    }

    fn layout(&self) -> Layout {
        self.root
            .and_then(|root| self.layout_at(root))
            .unwrap_or_else(Layout::empty)
    }

    fn layout_at(&self, id: NodeId) -> Option<Layout> {
        Some(match self.get(id)? {
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
                first: Box::new(self.layout_at(split.a)?),
                second: Box::new(self.layout_at(split.b)?),
            },
        })
    }
}

/// A layout being put into `tree`, with the panels `known` and how
/// many areas it has made.
struct Arranging<'a, F> {
    tree: &'a mut DockTree,
    known: &'a F,
    areas: usize,
}

impl<F: Fn(&str) -> bool> Arranging<'_, F> {
    fn build(&mut self, layout: &Layout) -> Option<NodeId> {
        match layout {
            Layout::Tabs { panels, shown } => {
                let front = panels.get(*shown);
                let windows = panels
                    .iter()
                    .filter(|panel| (self.known)(panel.as_str()))
                    .map(|panel| panel.as_str().to_owned())
                    .collect::<Vec<_>>();
                if windows.is_empty() {
                    return None;
                }
                let at = front
                    .and_then(|front| {
                        windows.iter().position(|window| {
                            window == front.as_str()
                        })
                    })
                    .unwrap_or(0);
                let leaf = DockLeaf::new(
                    format!("area_{}", self.areas),
                    DockAreaStyle::TabBar,
                )
                .with_windows(windows);
                self.areas += 1;
                let id = self.tree.insert(DockNode::Leaf(leaf));
                if let Some(DockNode::Leaf(leaf)) =
                    self.tree.get_mut(id)
                {
                    leaf.active =
                        leaf.windows.get(at).map(|tab| tab.id);
                }
                Some(id)
            }
            Layout::Split {
                axis,
                share,
                first,
                second,
            } => {
                let a = self.build(first);
                let b = self.build(second);
                match (a, b) {
                    (Some(a), Some(b)) => Some(self.tree.insert(
                        DockNode::Split(DockSplit {
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
                        }),
                    )),
                    (one, other) => one.or(other),
                }
            }
        }
    }
}
