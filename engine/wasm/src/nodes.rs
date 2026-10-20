//! The nodes of a plugin's retained panels, as the plugin makes,
//! sets, orders and drops them in a call: kept as changes to make to
//! the engine's widgets once the call ends, each held to the budgets.

use core::fmt;
use std::collections::HashSet;

use wasmtime::component::Resource;

use crate::instance::State;
use crate::instance::ui_bindings::hamlet::engine::ui::{
    Host, HostNode, Kind, Motion, Property,
};

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

/// The nodes a plugin holds, and what it did to them in the call
/// under way.
#[derive(Default)]
pub(crate) struct Nodes {
    next: u32,
    held: HashSet<u32>,
    pub(crate) changes: Vec<NodeChange>,
    /// Makes, sets and orders in the call.
    pub(crate) spent: usize,
    /// Calls on nodes in the call, of every kind.
    pub(crate) calls: u64,
}

impl Nodes {
    /// A node of the host's own, handed to the plugin as the root of
    /// a panel.
    pub(crate) fn root(&mut self) -> Resource<UiNode> {
        Resource::new_own(self.number())
    }

    fn number(&mut self) -> u32 {
        self.next += 1;
        self.held.insert(self.next);
        self.next
    }

    /// Counts one change within `most`.
    fn spend(&mut self, most: usize) -> wasmtime::Result<()> {
        self.calls += 1;
        if self.spent >= most {
            return Err(wasmtime::Error::new(Over::Changes));
        }
        self.spent += 1;
        Ok(())
    }
}

/// A plugin went past a budget of its nodes.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Over {
    Changes,
    Nodes,
}

impl fmt::Display for Over {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Changes => {
                write!(f, "it changed more nodes than one call may")
            }
            Self::Nodes => {
                write!(f, "it held more nodes than it may")
            }
        }
    }
}

impl core::error::Error for Over {}

impl Host for State {}

impl HostNode for State {
    fn new(
        &mut self,
        kind: Kind,
        parent: Resource<UiNode>,
        key: u32,
    ) -> wasmtime::Result<Resource<UiNode>> {
        self.nodes.spend(self.caps.node_changes)?;
        if self.nodes.held.len() >= self.caps.nodes {
            return Err(wasmtime::Error::new(Over::Nodes));
        }
        let node = self.nodes.number();
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
        if self.nodes.held.remove(&node.rep()) {
            self.nodes.changes.push(NodeChange::Drop(node.rep()));
        }
        Ok(())
    }
}
