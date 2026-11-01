//! Changes to a sheet, each giving the change that undoes it.

use bevy_math::Vec2;
use serde::{Deserialize, Serialize};

use crate::graph::{
    GraphInput, GraphOutput, Group, Node, NodeId, Setting, Sheet,
    Wire,
};

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
    /// Gives the graph an input, set by the graph above.
    AddInput(GraphInput),
    /// Takes an input from the graph, and puts every node set from
    /// it back to its default.
    RemoveInput(String),
    /// Puts `input` in place of the input named `name`. A new name is
    /// carried to every node set from it.
    ChangeInput {
        name: String,
        input: GraphInput,
    },
    /// Gives the graph an output, from a node's port.
    AddOutput(GraphOutput),
    RemoveOutput(String),
    /// Puts a group at `at` among the sheet's groups.
    AddGroup {
        at: usize,
        group: Group,
    },
    RemoveGroup(usize),
    /// Puts `group` in place of the group at `at`.
    ChangeGroup {
        at: usize,
        group: Group,
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
    #[error("the graph already has a port named {0}")]
    NameTaken(String),
    #[error("the graph has no port named {0}")]
    NoName(String),
    #[error("no group is at {0}")]
    NoGroup(usize),
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
                for (at, group) in self.groups.iter_mut().enumerate()
                {
                    if group.nodes.contains(&id) {
                        undo.push(Edit::ChangeGroup {
                            at,
                            group: group.clone(),
                        });
                        group.nodes.remove(&id);
                    }
                }
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
            Edit::AddInput(input) => {
                if graph
                    .inputs
                    .iter()
                    .any(|other| other.name == input.name)
                {
                    return Err(EditRefusal::NameTaken(input.name));
                }
                let name = input.name.clone();
                graph.inputs.push(input);
                Ok(Edit::RemoveInput(name))
            }
            Edit::RemoveInput(name) => {
                let index = graph
                    .inputs
                    .iter()
                    .position(|input| input.name == name)
                    .ok_or_else(|| {
                        EditRefusal::NoName(name.clone())
                    })?;
                let mut undo =
                    vec![Edit::AddInput(graph.inputs.remove(index))];
                let from = Setting::FromParent(name);
                for node in &mut graph.nodes {
                    node.settings.retain(|input, setting| {
                        if *setting != from {
                            return true;
                        }
                        undo.push(Edit::Set {
                            node: node.id,
                            input: input.clone(),
                            setting: Some(setting.clone()),
                        });
                        false
                    });
                }
                Ok(Edit::Batch(undo))
            }
            Edit::ChangeInput { name, input } => {
                let index = graph
                    .inputs
                    .iter()
                    .position(|other| other.name == name)
                    .ok_or_else(|| {
                        EditRefusal::NoName(name.clone())
                    })?;
                if input.name != name
                    && graph
                        .inputs
                        .iter()
                        .any(|other| other.name == input.name)
                {
                    return Err(EditRefusal::NameTaken(input.name));
                }
                if input.name != name {
                    for setting in graph
                        .nodes
                        .iter_mut()
                        .flat_map(|node| node.settings.values_mut())
                    {
                        if *setting
                            == Setting::FromParent(name.clone())
                        {
                            *setting = Setting::FromParent(
                                input.name.clone(),
                            );
                        }
                    }
                }
                let new_name = input.name.clone();
                let old = std::mem::replace(
                    &mut graph.inputs[index],
                    input,
                );
                Ok(Edit::ChangeInput {
                    name: new_name,
                    input: old,
                })
            }
            Edit::AddOutput(output) => {
                if graph
                    .outputs
                    .iter()
                    .any(|other| other.name == output.name)
                {
                    return Err(EditRefusal::NameTaken(output.name));
                }
                let name = output.name.clone();
                graph.outputs.push(output);
                Ok(Edit::RemoveOutput(name))
            }
            Edit::RemoveOutput(name) => {
                let index = graph
                    .outputs
                    .iter()
                    .position(|output| output.name == name)
                    .ok_or(EditRefusal::NoName(name))?;
                Ok(Edit::AddOutput(graph.outputs.remove(index)))
            }
            Edit::AddGroup { at, group } => {
                if at > self.groups.len() {
                    return Err(EditRefusal::NoGroup(at));
                }
                self.groups.insert(at, group);
                Ok(Edit::RemoveGroup(at))
            }
            Edit::RemoveGroup(at) => {
                if at >= self.groups.len() {
                    return Err(EditRefusal::NoGroup(at));
                }
                Ok(Edit::AddGroup {
                    at,
                    group: self.groups.remove(at),
                })
            }
            Edit::ChangeGroup { at, group } => {
                let old = self
                    .groups
                    .get_mut(at)
                    .ok_or(EditRefusal::NoGroup(at))?;
                Ok(Edit::ChangeGroup {
                    at,
                    group: std::mem::replace(old, group),
                })
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
