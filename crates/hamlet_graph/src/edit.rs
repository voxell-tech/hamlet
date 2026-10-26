//! Changes to a sheet, each giving the change that undoes it.

use bevy_math::Vec2;
use serde::{Deserialize, Serialize};

use crate::graph::{Node, NodeId, Setting, Sheet, Wire};

/// A change to a [`Sheet`].
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub enum Edit {
    /// Adds a node, drawn at `at` if given.
    AddNode {
        node: Node,
        at: Option<Vec2>,
    },
    RemoveNode(NodeId),
    Connect(Wire),
    Disconnect(Wire),
    /// Sets how an input is set, or clears it to its kind's initial
    /// value.
    Set {
        node: NodeId,
        input: String,
        setting: Option<Setting>,
    },
    /// Moves where a node is drawn, or forgets it.
    Move {
        node: NodeId,
        to: Option<Vec2>,
    },
    /// Edits applied in order, all or none.
    Batch(Vec<Edit>),
}

/// Why an [`Edit`] does not apply.
#[derive(Clone, PartialEq, Eq, Debug, thiserror::Error)]
pub enum EditRefusal {
    #[error("no node is {0}")]
    NoNode(NodeId),
    #[error("a node is already {0}")]
    Taken(NodeId),
    #[error("no such wire")]
    NoWire,
    #[error("the input already has a wire")]
    Wired,
}

impl Sheet {
    /// Applies `edit`, giving the edit that undoes it.
    pub fn apply(&mut self, edit: Edit) -> Result<Edit, EditRefusal> {
        let graph = &mut self.graph;
        match edit {
            Edit::AddNode { node, at } => {
                if graph.node(node.id).is_some() {
                    return Err(EditRefusal::Taken(node.id));
                }
                let id = node.id;
                graph.nodes.push(node);
                if let Some(at) = at {
                    self.layout.insert(id, at);
                }
                Ok(Edit::RemoveNode(id))
            }
            Edit::RemoveNode(id) => {
                let index = graph
                    .nodes
                    .iter()
                    .position(|node| node.id == id)
                    .ok_or(EditRefusal::NoNode(id))?;
                let node = graph.nodes.remove(index);
                let at = self.layout.remove(&id);
                let (cut, kept) = std::mem::take(&mut graph.wires)
                    .into_iter()
                    .partition::<Vec<_>, _>(|wire| {
                        wire.from.node == id || wire.to.node == id
                    });
                graph.wires = kept;
                let mut undo = vec![Edit::AddNode { node, at }];
                undo.extend(cut.into_iter().map(Edit::Connect));
                Ok(Edit::Batch(undo))
            }
            Edit::Connect(wire) => {
                for end in [&wire.from, &wire.to] {
                    if graph.node(end.node).is_none() {
                        return Err(EditRefusal::NoNode(end.node));
                    }
                }
                if graph.wires.iter().any(|other| other.to == wire.to)
                {
                    return Err(EditRefusal::Wired);
                }
                graph.wires.push(wire.clone());
                Ok(Edit::Disconnect(wire))
            }
            Edit::Disconnect(wire) => {
                let index = graph
                    .wires
                    .iter()
                    .position(|other| *other == wire)
                    .ok_or(EditRefusal::NoWire)?;
                graph.wires.remove(index);
                Ok(Edit::Connect(wire))
            }
            Edit::Set {
                node,
                input,
                setting,
            } => {
                let found = graph
                    .nodes
                    .iter_mut()
                    .find(|other| other.id == node)
                    .ok_or(EditRefusal::NoNode(node))?;
                let old = match setting {
                    Some(setting) => {
                        found.settings.insert(input.clone(), setting)
                    }
                    None => found.settings.remove(&input),
                };
                Ok(Edit::Set {
                    node,
                    input,
                    setting: old,
                })
            }
            Edit::Move { node, to } => {
                if graph.node(node).is_none() {
                    return Err(EditRefusal::NoNode(node));
                }
                let old = match to {
                    Some(to) => self.layout.insert(node, to),
                    None => self.layout.remove(&node),
                };
                Ok(Edit::Move { node, to: old })
            }
            Edit::Batch(edits) => {
                let mut undo = Vec::with_capacity(edits.len());
                for edit in edits {
                    match self.apply(edit) {
                        Ok(inverse) => undo.push(inverse),
                        Err(refusal) => {
                            for inverse in undo.into_iter().rev() {
                                // An inverse undoes what just
                                // applied, so it applies.
                                let _ = self.apply(inverse);
                            }
                            return Err(refusal);
                        }
                    }
                }
                undo.reverse();
                Ok(Edit::Batch(undo))
            }
        }
    }
}
