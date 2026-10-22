//! The dock's tree: a panel brought to the front, and a workspace's
//! layout put into it and read back out of it.

use std::collections::HashSet;
use std::mem;

use bevy::platform::collections::HashMap;
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
    /// areas left with none. An area keeps its id while it holds a
    /// panel it held, a panel its tab's id, and a split its id while
    /// it splits the same two, so their views are kept; anything else
    /// is given a new id.
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
        let old = mem::take(&mut self.nodes);
        let old_root = self.root.take();
        let mut arranging = Arranging {
            tree: self,
            known: &known,
            areas: 0,
            old: &old,
            kept: HashSet::new(),
        };
        let root = arranging.build(layout).unwrap_or_else(|| {
            let empty = old_root.filter(|root| {
                old.get(root)
                    .and_then(DockNode::as_leaf)
                    .is_some_and(|leaf| leaf.windows.is_empty())
            });
            let leaf = DockLeaf::new("area_0", DockAreaStyle::TabBar);
            arranging.place(DockNode::Leaf(leaf), empty)
        });
        self.root = Some(root);
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

/// A layout being put into `tree`, with the panels `known`, how many
/// areas it has made, the nodes the tree had before and those of them
/// kept.
struct Arranging<'a, F> {
    tree: &'a mut DockTree,
    known: &'a F,
    areas: usize,
    old: &'a HashMap<NodeId, DockNode>,
    kept: HashSet<NodeId>,
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
                // The first area not yet kept that held one of these
                // panels.
                let was = self
                    .old
                    .iter()
                    .filter(|(id, _)| !self.kept.contains(id))
                    .filter_map(|(id, node)| {
                        Some((*id, node.as_leaf()?))
                    })
                    .filter(|(_, leaf)| {
                        windows
                            .iter()
                            .any(|window| leaf.has_window(window))
                    })
                    .min_by_key(|(id, _)| id.0);
                let mut leaf = DockLeaf::new(
                    format!("area_{}", self.areas),
                    DockAreaStyle::TabBar,
                )
                .with_windows(windows);
                if let Some((_, old)) = was {
                    let mut left = old.windows.clone();
                    for tab in &mut leaf.windows {
                        let held = left.iter().position(|old| {
                            old.window_id == tab.window_id
                        });
                        if let Some(at) = held {
                            tab.id = left.remove(at).id;
                        }
                    }
                }
                self.areas += 1;
                let id = self.place(
                    DockNode::Leaf(leaf),
                    was.map(|(id, _)| id),
                );
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
                let (Some(a), Some(b)) = (a, b) else {
                    return a.or(b);
                };
                let axis = match axis {
                    Axis::Horizontal => SplitAxis::Horizontal,
                    Axis::Vertical => SplitAxis::Vertical,
                };
                let was = self
                    .old
                    .iter()
                    .find(|(_, node)| {
                        node.as_split().is_some_and(|split| {
                            (split.axis, split.a, split.b)
                                == (axis, a, b)
                        })
                    })
                    .map(|(id, _)| *id);
                let split = DockSplit {
                    axis,
                    fraction: share
                        .clamp(LEAST_SHARE, 1.0 - LEAST_SHARE),
                    a,
                    b,
                };
                Some(self.place(DockNode::Split(split), was))
            }
        }
    }

    /// Puts `node` in the tree, under the id `was` had when given
    /// one.
    fn place(
        &mut self,
        node: DockNode,
        was: Option<NodeId>,
    ) -> NodeId {
        let made = self.tree.insert(node);
        let Some(was) = was else {
            return made;
        };
        if let Some(node) = self.tree.nodes.remove(&made) {
            self.tree.nodes.insert(was, node);
        }
        self.kept.insert(was);
        was
    }
}
