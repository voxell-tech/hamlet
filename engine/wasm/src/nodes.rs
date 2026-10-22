//! The nodes of a plugin's retained panels, as the plugin makes,
//! sets, orders and drops them in a call: kept as changes to make to
//! the engine's widgets once the call ends, each held to the budgets
//! and to what its kind may hold and be set to.

use core::fmt;
use std::collections::HashMap;

use wasmtime::component::Resource;

use crate::instance::State;
use crate::instance::ui_bindings::hamlet::engine::ui::{
    Host, HostNode, Kind, Motion, Property, StatusItem,
};
use crate::surfaces::SurfaceChange;

/// A node of a retained panel, as the plugin holds it.
pub struct UiNode;

/// A change to a plugin's widgets, by the number the host gave each
/// node.
#[derive(Clone, Debug)]
pub enum NodeChange {
    Make {
        node: u32,
        /// The plugin's name for it, in the inputs it is given.
        key: u32,
        kind: Kind,
        parent: u32,
    },
    Set {
        node: u32,
        value: Property,
        motion: Option<Motion>,
    },
    Order {
        node: u32,
        children: Vec<u32>,
    },
    Watch {
        node: u32,
        hover: bool,
        press: bool,
    },
    Drop(u32),
}

/// A node the plugin holds: its kind, and how deep it sits under its
/// panel's root.
#[derive(Clone, Copy, Debug)]
struct Held {
    kind: Kind,
    depth: usize,
}

/// The nodes a plugin holds, and what it did to them in the call
/// under way.
#[derive(Default)]
pub(crate) struct Nodes {
    next: u32,
    held: HashMap<u32, Held>,
    pub(crate) changes: Vec<NodeChange>,
    /// Makes, sets and orders in the call.
    pub(crate) spent: usize,
    /// Calls on nodes in the call, of every kind.
    pub(crate) calls: u64,
}

impl Nodes {
    /// A node of the host's own, handed to the plugin as the root of
    /// a panel: a frame.
    pub(crate) fn root(&mut self) -> Resource<UiNode> {
        Resource::new_own(self.number(Held {
            kind: Kind::Frame,
            depth: 0,
        }))
    }

    fn number(&mut self, held: Held) -> u32 {
        self.next += 1;
        self.held.insert(self.next, held);
        self.next
    }

    /// Counts one change within `most`.
    fn spend(&mut self, most: usize) -> wasmtime::Result<()> {
        self.calls += 1;
        if self.spent >= most {
            return Err(wasmtime::Error::new(NodeError::Changes));
        }
        self.spent += 1;
        Ok(())
    }

    fn kind(&self, node: u32) -> wasmtime::Result<Kind> {
        self.held
            .get(&node)
            .map(|held| held.kind)
            .ok_or_else(|| wasmtime::Error::new(NodeError::Unknown))
    }
}

impl Kind {
    /// Whether a node of this kind holds others.
    pub(crate) fn holds(self) -> bool {
        matches!(
            self,
            Self::Frame | Self::Button | Self::List | Self::Scroll
        )
    }

    /// Whether `value` is a property of this kind.
    fn takes(self, value: &Property) -> bool {
        match value {
            Property::Text(_) => matches!(
                self,
                Self::Label | Self::Button | Self::Icon | Self::Field
            ),
            Property::Tone(_) => {
                matches!(self, Self::Label | Self::Icon)
            }
            Property::Size(_)
            | Property::Bold(_)
            | Property::Face(_) => self == Self::Label,
            Property::Opacity(_)
            | Property::Offset(_)
            | Property::Tip(_)
            | Property::Menu(_) => true,
            Property::Style(_) | Property::Folder(_) => {
                self == Self::Button
            }
            Property::Direction(_)
            | Property::Space(_)
            | Property::Align(_)
            | Property::Fill(_) => self == Self::Frame,
            Property::Placeholder(_) | Property::Lines(_) => {
                self == Self::Field
            }
            Property::On(_) => self == Self::Toggle,
            Property::Stick(_) => self == Self::Scroll,
            Property::Options(_) | Property::Selected(_) => {
                self == Self::Dropdown
            }
        }
    }
}

/// A plugin went past a budget of its nodes, or misused one.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum NodeError {
    Changes,
    Nodes,
    Depth,
    /// A node made under one that holds none.
    Leaf,
    /// A property set on a node whose kind has none such.
    Property,
    /// A node it never held.
    Unknown,
}

impl fmt::Display for NodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Changes => {
                write!(f, "it changed more nodes than one call may")
            }
            Self::Nodes => {
                write!(f, "it held more nodes than it may")
            }
            Self::Depth => {
                write!(f, "it nested nodes deeper than it may")
            }
            Self::Leaf => {
                write!(f, "it put a node under one that holds none")
            }
            Self::Property => {
                write!(f, "it set a property a node does not have")
            }
            Self::Unknown => {
                write!(f, "it named a node it never made")
            }
        }
    }
}

impl core::error::Error for NodeError {}

impl Host for State {
    fn set_status(
        &mut self,
        name: String,
        item: StatusItem,
    ) -> wasmtime::Result<()> {
        self.nodes.spend(self.caps.node_changes)?;
        self.surfaces.push(SurfaceChange::Status { name, item });
        Ok(())
    }

    fn show_overlay(
        &mut self,
        name: String,
        shown: bool,
    ) -> wasmtime::Result<()> {
        self.nodes.spend(self.caps.node_changes)?;
        self.surfaces.push(SurfaceChange::Overlay { name, shown });
        Ok(())
    }

    fn open(&mut self, name: String) -> wasmtime::Result<()> {
        self.nodes.spend(self.caps.node_changes)?;
        self.surfaces.push(SurfaceChange::Open(name));
        Ok(())
    }
}

impl HostNode for State {
    fn new(
        &mut self,
        kind: Kind,
        parent: Resource<UiNode>,
        key: u32,
    ) -> wasmtime::Result<Resource<UiNode>> {
        self.nodes.spend(self.caps.node_changes)?;
        if self.nodes.held.len() >= self.caps.nodes {
            return Err(wasmtime::Error::new(NodeError::Nodes));
        }
        let Some(&above) = self.nodes.held.get(&parent.rep()) else {
            return Err(wasmtime::Error::new(NodeError::Unknown));
        };
        if !above.kind.holds() {
            return Err(wasmtime::Error::new(NodeError::Leaf));
        }
        if above.depth >= self.caps.node_depth {
            return Err(wasmtime::Error::new(NodeError::Depth));
        }
        let node = self.nodes.number(Held {
            kind,
            depth: above.depth + 1,
        });
        self.nodes.changes.push(NodeChange::Make {
            node,
            key,
            kind,
            parent: parent.rep(),
        });
        Ok(Resource::new_own(node))
    }

    fn set(
        &mut self,
        node: Resource<UiNode>,
        value: Property,
        motion: Option<Motion>,
    ) -> wasmtime::Result<()> {
        self.nodes.spend(self.caps.node_changes)?;
        if !self.nodes.kind(node.rep())?.takes(&value) {
            return Err(wasmtime::Error::new(NodeError::Property));
        }
        self.nodes.changes.push(NodeChange::Set {
            node: node.rep(),
            value,
            motion,
        });
        Ok(())
    }

    fn reorder(
        &mut self,
        node: Resource<UiNode>,
        children: Vec<Resource<UiNode>>,
    ) -> wasmtime::Result<()> {
        self.nodes.spend(self.caps.node_changes)?;
        self.nodes.changes.push(NodeChange::Order {
            node: node.rep(),
            children: children.iter().map(Resource::rep).collect(),
        });
        Ok(())
    }

    fn watch(
        &mut self,
        node: Resource<UiNode>,
        hover: bool,
        press: bool,
    ) -> wasmtime::Result<()> {
        self.nodes.spend(self.caps.node_changes)?;
        self.nodes.changes.push(NodeChange::Watch {
            node: node.rep(),
            hover,
            press,
        });
        Ok(())
    }

    fn drop(
        &mut self,
        node: Resource<UiNode>,
    ) -> wasmtime::Result<()> {
        self.nodes.calls += 1;
        if self.nodes.held.remove(&node.rep()).is_some() {
            self.nodes.changes.push(NodeChange::Drop(node.rep()));
        }
        Ok(())
    }
}
