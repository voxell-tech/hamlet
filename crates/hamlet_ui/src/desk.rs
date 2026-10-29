//! Desks: named arrangements of the dock's panels, kept with the
//! view. One is shown at a time, and what is changed in the dock by
//! hand is kept in it.

mod switcher;

use std::collections::HashSet;
use std::mem;

use bevy::app::{App, Update};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::schedule::common_conditions::resource_changed;
use bevy::ecs::system::{Res, ResMut};
use bevy::platform::collections::HashMap;
use bevy_fynix::dock::{
    DockAreaStyle, DockLeaf, DockNode, DockRegistry, DockSplit,
    DockTree, NodeId, SplitAxis,
};
use serde::{Deserialize, Serialize};

pub(crate) use self::switcher::{popups, switcher};
use crate::dock;
use crate::theme::HamletTheme;

/// The name of the first desk.
const FIRST: &str = "Main";

/// The most characters a desk's name may have.
const LONGEST_NAME: usize = 40;

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Desks>()
        .init_resource::<Shown>()
        .add_plugins(switcher::plugin)
        .add_systems(
            Update,
            (
                show.run_if(resource_changed::<Desks>),
                capture.run_if(resource_changed::<DockTree>),
            )
                .chain(),
        );
}

/// One node of a dock, with its children named by their place in
/// the list.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub(crate) enum Part {
    Tabs {
        area: String,
        tabs: Vec<String>,
        /// The place of the tab in front.
        front: usize,
        headless: bool,
        /// It stays when its last tab is closed.
        kept: bool,
    },
    Split {
        /// `a` is above `b`. Beside it otherwise.
        stacked: bool,
        /// The share of the split that `a` takes.
        share: f32,
        a: usize,
        b: usize,
    },
}

/// A named arrangement of the dock. The first part of `dock` is its
/// root; a dock of no parts is the one the window starts with.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub(crate) struct Desk {
    pub(crate) name: String,
    pub(crate) dock: Vec<Part>,
}

/// The chief's desks, and the one shown. There is always at least
/// one.
#[derive(
    Resource, Serialize, Deserialize, Clone, PartialEq, Debug,
)]
pub(crate) struct Desks {
    shown: usize,
    list: Vec<Desk>,
}

impl Default for Desks {
    fn default() -> Self {
        Self::first(Vec::new())
    }
}

impl Desks {
    /// One desk, holding `dock`.
    pub(crate) fn first(dock: Vec<Part>) -> Self {
        Self {
            shown: 0,
            list: vec![Desk {
                name: FIRST.into(),
                dock,
            }],
        }
    }

    pub(crate) fn list(&self) -> &[Desk] {
        &self.list
    }

    /// The index of the desk shown.
    pub(crate) fn shown(&self) -> usize {
        self.shown
    }

    /// Shows the desk at `at`, if there is one.
    pub(crate) fn show(&mut self, at: usize) {
        if at < self.list.len() && at != self.shown {
            self.shown = at;
        }
    }

    /// Shows the next desk, or the previous with `back`, going
    /// round.
    pub(crate) fn cycle(&mut self, back: bool) {
        let count = self.list.len();
        let step = if back { count - 1 } else { 1 };
        self.show((self.shown + step) % count);
    }

    /// Adds a desk with a name of its own and the dock the window
    /// starts with, shows it and gives its index.
    pub(crate) fn add(&mut self) -> usize {
        let name = (self.list.len() + 1..)
            .map(|n| format!("Desk {n}"))
            .find(|name| !self.named(name, None))
            .unwrap_or_default();
        let at = self.list.len();
        self.list.push(Desk {
            name,
            dock: Vec::new(),
        });
        self.shown = at;
        at
    }

    /// Adds a copy of the desk at `at` after it, numbered, and shows
    /// it.
    pub(crate) fn duplicate(&mut self, at: usize) {
        let Some(desk) = self.list.get(at).cloned() else {
            return;
        };
        let name = self.free_name(&desk.name);
        self.list.insert(at + 1, Desk { name, ..desk });
        self.shown = at + 1;
    }

    /// Moves the desk at `from` to `to`, the shown one still shown.
    pub(crate) fn move_to(&mut self, from: usize, to: usize) {
        let count = self.list.len();
        if from >= count || to >= count || from == to {
            return;
        }
        let shown = self.shown;
        let desk = self.list.remove(from);
        self.list.insert(to, desk);
        self.shown = match shown {
            _ if shown == from => to,
            _ if from < shown && shown <= to => shown - 1,
            _ if to <= shown && shown < from => shown + 1,
            _ => shown,
        };
    }

    /// Names the desk at `at` `name`, trimmed, unless that is empty,
    /// too long or another's.
    pub(crate) fn rename(&mut self, at: usize, name: &str) {
        let name = name.trim();
        let fits = !name.is_empty()
            && name.chars().count() <= LONGEST_NAME
            && !self.named(name, Some(at));
        if let Some(desk) = self.list.get_mut(at)
            && fits
        {
            desk.name = name.to_owned();
        }
    }

    /// Closes the desk at `at`, unless it is the last one.
    pub(crate) fn close(&mut self, at: usize) {
        if self.list.len() < 2 || at >= self.list.len() {
            return;
        }
        self.list.remove(at);
        if self.shown > at || self.shown == self.list.len() {
            self.shown -= 1;
        }
    }

    /// `name`, trimmed and numbered when another desk has it.
    fn free_name(&self, name: &str) -> String {
        let name = name
            .trim()
            .chars()
            .take(LONGEST_NAME - 3)
            .collect::<String>();
        (1..)
            .map(|n| match n {
                1 => name.clone(),
                n => format!("{name} {n}"),
            })
            .find(|name| !name.is_empty() && !self.named(name, None))
            .unwrap_or_default()
    }

    /// Whether a desk other than the one at `besides` is called
    /// `name`.
    fn named(&self, name: &str, besides: Option<usize>) -> bool {
        self.list.iter().enumerate().any(|(at, desk)| {
            Some(at) != besides
                && desk.name.eq_ignore_ascii_case(name)
        })
    }

    /// These, with at least one desk, one of them shown, and every
    /// one named.
    pub(crate) fn cleaned(mut self) -> Self {
        if self.list.is_empty() {
            return Self::default();
        }
        self.shown = self.shown.min(self.list.len() - 1);
        for (at, desk) in self.list.iter_mut().enumerate() {
            if desk.name.trim().is_empty() {
                desk.name = format!("Desk {}", at + 1);
            }
        }
        self
    }
}

/// What the dock was last given, to tell a change made by hand from
/// one made here.
#[derive(Resource, Default)]
pub(crate) struct Shown {
    desk: Option<usize>,
    /// The desk's dock as it was laid out.
    saved: Vec<Part>,
    /// The dock as the tree had it then, without the panels it does
    /// not know.
    drawn: Vec<Part>,
}

/// Lays the shown desk out in the dock when another is shown: the
/// dock the window starts with when it lays out nothing.
pub(crate) fn show(
    mut desks: ResMut<Desks>,
    registry: Res<DockRegistry<HamletTheme>>,
    mut tree: ResMut<DockTree>,
    mut shown: ResMut<Shown>,
) {
    let at = desks.shown;
    let Some(desk) = desks.list.get(at) else {
        return;
    };
    if shown.desk == Some(at) && shown.saved == desk.dock {
        return;
    }
    let known = |id: &str| registry.get(id).is_some();
    if !lay_out(&mut tree, &desk.dock, &known) {
        dock::designed(&mut tree);
        desks.list[at].dock = noted(&tree);
    }
    *shown = Shown {
        desk: Some(at),
        saved: desks.list[at].dock.clone(),
        drawn: noted(&tree),
    };
}

/// Keeps in the shown desk what was changed in the dock by hand.
pub(crate) fn capture(
    tree: Res<DockTree>,
    mut shown: ResMut<Shown>,
    mut desks: ResMut<Desks>,
) {
    let now = noted(&tree);
    if shown.drawn == now {
        return;
    }
    let Some(at) = shown.desk else {
        return;
    };
    shown.drawn.clone_from(&now);
    shown.saved.clone_from(&now);
    if let Some(desk) = desks.list.get_mut(at) {
        desk.dock = now;
    }
}

/// Writes the node `id` and all under it to `parts`, and gives its
/// place there.
fn note(
    tree: &DockTree,
    id: NodeId,
    parts: &mut Vec<Part>,
) -> Option<usize> {
    let at = parts.len();
    match tree.get(id)? {
        DockNode::Leaf(leaf) => {
            let front = leaf
                .active
                .and_then(|tab| leaf.tab_index(tab))
                .unwrap_or_default();
            parts.push(Part::Tabs {
                area: leaf.area_id.clone(),
                tabs: leaf
                    .tabs()
                    .map(|(window, _)| window.to_string())
                    .collect(),
                front,
                headless: leaf.style == DockAreaStyle::Headless,
                kept: leaf.is_persistent(),
            });
        }
        DockNode::Split(split) => {
            let (stacked, share) =
                (split.axis == SplitAxis::Vertical, split.fraction);
            let (first, second) = (split.a, split.b);
            // Held open until its children have their places.
            parts.push(Part::Split {
                stacked,
                share,
                a: 0,
                b: 0,
            });
            let a = note(tree, first, parts)?;
            let b = note(tree, second, parts)?;
            parts[at] = Part::Split {
                stacked,
                share,
                a,
                b,
            };
        }
    }
    Some(at)
}

/// The dock `tree` holds, as parts.
pub(crate) fn noted(tree: &DockTree) -> Vec<Part> {
    let mut parts = Vec::new();
    match tree.root.and_then(|root| note(tree, root, &mut parts)) {
        Some(_) => parts,
        None => Vec::new(),
    }
}

/// Lays `parts` out in `tree`, leaving out the panels not `known`.
/// An area keeps its id while it holds a panel it held, a panel its
/// tab's id, and a split its id while it splits the same two, so
/// their views are kept. Whether `parts` laid anything out; the tree
/// is left empty when not.
pub(crate) fn lay_out(
    tree: &mut DockTree,
    parts: &[Part],
    known: &impl Fn(&str) -> bool,
) -> bool {
    let old = mem::take(&mut tree.nodes);
    tree.root = None;
    let mut laying = Laying {
        tree,
        known,
        old: &old,
        kept: HashSet::new(),
    };
    let root = laying.build(parts, 0, 0);
    tree.root = root;
    root.is_some()
}

/// Parts being laid out in `tree`, with the panels `known`, the
/// nodes the tree had before and those of them kept.
struct Laying<'a, F> {
    tree: &'a mut DockTree,
    known: &'a F,
    old: &'a HashMap<NodeId, DockNode>,
    kept: HashSet<NodeId>,
}

impl<F: Fn(&str) -> bool> Laying<'_, F> {
    /// Builds the part at `at`. A tab whose panel is not known is
    /// left out, a tab bar left empty is dropped, and a split that
    /// loses a side becomes its other side. `depth` stops a list that
    /// names itself.
    fn build(
        &mut self,
        parts: &[Part],
        at: usize,
        depth: usize,
    ) -> Option<NodeId> {
        if depth > parts.len() {
            return None;
        }
        match parts.get(at)? {
            Part::Tabs {
                area,
                tabs,
                front,
                headless,
                kept,
            } => {
                let front =
                    tabs.get(*front).filter(|tab| (self.known)(tab));
                let open = tabs
                    .iter()
                    .filter(|tab| (self.known)(tab))
                    .cloned()
                    .collect::<Vec<_>>();
                if open.is_empty() && !kept {
                    return None;
                }
                let shown = front
                    .and_then(|tab| {
                        open.iter().position(|t| t == tab)
                    })
                    .unwrap_or_default();
                let style = if *headless {
                    DockAreaStyle::Headless
                } else {
                    DockAreaStyle::TabBar
                };
                let mut leaf = DockLeaf::new(area.clone(), style)
                    .with_windows(open);
                if *kept {
                    leaf = leaf.persistent();
                }
                // The first area not yet kept that held one of these
                // panels.
                let was = self
                    .old
                    .iter()
                    .filter(|(id, _)| !self.kept.contains(id))
                    .filter_map(|(id, node)| {
                        Some((*id, node.as_leaf()?))
                    })
                    .filter(|(_, old)| {
                        leaf.windows
                            .iter()
                            .any(|tab| old.has_window(&tab.window_id))
                    })
                    .min_by_key(|(id, _)| id.0);
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
                let id = self.place(
                    DockNode::Leaf(leaf),
                    was.map(|(id, _)| id),
                );
                if let Some(DockNode::Leaf(leaf)) =
                    self.tree.get_mut(id)
                {
                    leaf.active =
                        leaf.windows.get(shown).map(|tab| tab.id);
                }
                Some(id)
            }
            Part::Split {
                stacked,
                share,
                a,
                b,
            } => {
                let first = self.build(parts, *a, depth + 1);
                let second = self.build(parts, *b, depth + 1);
                let (Some(a), Some(b)) = (first, second) else {
                    return first.or(second);
                };
                let axis = if *stacked {
                    SplitAxis::Vertical
                } else {
                    SplitAxis::Horizontal
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
                let id = self.place(
                    DockNode::Split(DockSplit {
                        axis,
                        fraction: 0.5,
                        a,
                        b,
                    }),
                    was,
                );
                self.tree.set_fraction(id, *share);
                Some(id)
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

#[cfg(test)]
mod tests {
    use bevy_fynix::dock::Edge;

    use super::*;

    fn dock() -> DockTree {
        let mut tree = DockTree::new();
        let centre = tree.set_root_leaf(
            DockLeaf::new("centre", DockAreaStyle::TabBar)
                .with_windows(vec!["world".into()]),
        );
        let (queue, _) =
            tree.split(centre, Edge::Left, "needs".into()).unwrap();
        let parent = tree.parent_of(queue).unwrap();
        tree.set_fraction(parent, 0.2);
        let (side, agent) =
            tree.split(centre, Edge::Right, "agent".into()).unwrap();
        tree.add_tab(side, "skills");
        tree.set_active(side, agent);
        tree
    }

    #[test]
    fn a_dock_comes_back_as_it_was_noted_and_keeps_its_tabs() {
        let parts = noted(&dock());
        let mut tree = dock();
        let agent = tree.find_leaf_with_window("agent").unwrap();
        let tabs = tree.leaf(agent).unwrap().windows.clone();
        assert!(lay_out(&mut tree, &parts, &|_| true));
        assert_eq!(noted(&tree), parts);
        // The same area and tabs, so their views are kept.
        assert_eq!(tree.leaf(agent).unwrap().windows, tabs);
        let leaf = tree.leaf(agent).unwrap();
        assert_eq!(
            leaf.tab_index(tree.active(agent).unwrap()),
            Some(0)
        );
    }

    #[test]
    fn a_panel_that_is_gone_is_left_out() {
        let parts = noted(&dock());
        let mut tree = DockTree::new();
        assert!(lay_out(&mut tree, &parts, &|id| {
            id != "needs" && id != "agent"
        }));
        assert!(tree.find_leaf_with_window("needs").is_none());
        let side = tree.find_leaf_with_window("skills").unwrap();
        assert_eq!(tree.leaf(side).unwrap().tabs().count(), 1);
        assert!(tree.find_leaf_with_window("world").is_some());
        assert!(!lay_out(&mut tree, &parts, &|_| false));
    }

    #[test]
    fn a_list_that_names_itself_lays_out_nothing() {
        let parts = vec![Part::Split {
            stacked: false,
            share: 0.5,
            a: 0,
            b: 0,
        }];
        assert!(!lay_out(&mut DockTree::new(), &parts, &|_| true));
    }

    #[test]
    fn the_shown_desk_stays_shown_as_desks_move_and_close() {
        let mut desks = Desks::default();
        desks.add();
        desks.add();
        desks.show(1);
        desks.move_to(0, 2);
        assert_eq!(desks.list()[desks.shown()].name, "Desk 2");
        desks.close(2);
        assert_eq!(desks.list()[desks.shown()].name, "Desk 2");
        desks.close(desks.shown());
        assert_eq!(desks.list().len(), 1);
        desks.close(0);
        assert_eq!(desks.list().len(), 1);
    }
}
