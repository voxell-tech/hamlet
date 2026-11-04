//! Checking a ruleset against its kinds.

use std::collections::{BTreeMap, BTreeSet};

use crate::graph::{Does, Graph, Node, NodeId, Ruleset, Setting};
use crate::grows;
use crate::node::{PortSpec, Registry};
use crate::value::{Constant, ValueType};

/// Something wrong in a graph.
#[derive(Clone, PartialEq, Debug, thiserror::Error)]
pub enum Error {
    #[error("no graph is named {0}")]
    UnknownGraph(String),
    #[error("{node} is of an unknown kind, {kind}")]
    UnknownKind { node: NodeId, kind: String },
    #[error("no node is {0}")]
    UnknownNode(NodeId),
    #[error("two nodes are {0}")]
    SameId(NodeId),
    #[error("{node} has no port {port}")]
    UnknownPort { node: NodeId, port: String },
    #[error(
        "{node} is the graph input {name}, which the graph does not \
         have"
    )]
    UnknownInput { node: NodeId, name: String },
    #[error("{node}.{port} takes {expected:?}, not {found:?}")]
    WrongType {
        node: NodeId,
        port: String,
        expected: ValueType,
        found: ValueType,
    },
    #[error("{node}.{port} has two wires")]
    TwoWires { node: NodeId, port: String },
    #[error("{node}.{port} is not set")]
    Unset { node: NodeId, port: String },
    #[error("{node}.{port} {reason}")]
    Invalid {
        node: NodeId,
        port: String,
        reason: &'static str,
    },
    #[error("the graph input {0} is not given")]
    NotGiven(String),
    #[error("the wires go round in a loop through {0:?}")]
    Loop(Vec<NodeId>),
    #[error("the graph runs itself")]
    RunsItself,
    #[error("{node} puts down {asset}, which is in no catalogue")]
    UnknownAsset { node: NodeId, asset: String },
}

impl Error {
    /// The node the error is with, when it is with one.
    pub fn node(&self) -> Option<NodeId> {
        match self {
            Error::UnknownKind { node, .. }
            | Error::UnknownPort { node, .. }
            | Error::WrongType { node, .. }
            | Error::TwoWires { node, .. }
            | Error::Unset { node, .. }
            | Error::Invalid { node, .. }
            | Error::UnknownInput { node, .. }
            | Error::UnknownAsset { node, .. } => Some(*node),
            Error::UnknownNode(node) | Error::SameId(node) => {
                Some(*node)
            }
            _ => None,
        }
    }
}

/// An [`Error`] and the graph it is in.
#[derive(Clone, PartialEq, Debug, thiserror::Error)]
#[error("{graph}: {error}")]
pub struct GraphError {
    pub graph: String,
    pub error: Error,
}

/// A port's name and type.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PortInfo {
    pub name: String,
    pub value_type: ValueType,
}

/// The inputs and outputs of `node` in `graph`, from its kind, the
/// graph it runs or the graph input it gives. A growing input has a
/// port for each wire into it, and one more.
pub fn ports(
    registry: &Registry,
    ruleset: &Ruleset,
    graph: &Graph,
    node: &Node,
) -> Result<(Vec<PortInfo>, Vec<PortInfo>), Error> {
    match &node.does {
        Does::Kind(name) => {
            let kind = registry.get(name).ok_or_else(|| {
                Error::UnknownKind {
                    node: node.id,
                    kind: name.clone(),
                }
            })?;
            let info = |specs: &[PortSpec]| {
                specs
                    .iter()
                    .flat_map(|spec| {
                        let names = if spec.grows {
                            grows::ports_of(spec, graph, node.id)
                        } else {
                            vec![spec.name.to_owned()]
                        };
                        names.into_iter().map(|name| PortInfo {
                            name,
                            value_type: spec.value_type,
                        })
                    })
                    .collect()
            };
            Ok((info(kind.inputs), info(kind.outputs)))
        }
        Does::Graph(name) => {
            let runs = ruleset
                .graphs
                .get(name)
                .ok_or_else(|| Error::UnknownGraph(name.clone()))?;
            let inputs = runs
                .inputs
                .iter()
                .map(|input| PortInfo {
                    name: input.name.clone(),
                    value_type: input.value_type,
                })
                .collect();
            let outputs = runs
                .outputs
                .iter()
                .map(|output| PortInfo {
                    name: output.name.clone(),
                    value_type: output.value_type,
                })
                .collect();
            Ok((inputs, outputs))
        }
        Does::Input(name) => {
            let input = graph
                .inputs
                .iter()
                .find(|input| &input.name == name)
                .ok_or_else(|| Error::UnknownInput {
                    node: node.id,
                    name: name.clone(),
                })?;
            Ok((
                Vec::new(),
                vec![PortInfo {
                    name: Does::INPUT_PORT.to_owned(),
                    value_type: input.value_type,
                }],
            ))
        }
    }
}

/// Every error in `ruleset` that can be found without running it.
pub fn validate(
    registry: &Registry,
    ruleset: &Ruleset,
) -> Vec<GraphError> {
    let mut errors = Vec::new();
    if !ruleset.graphs.contains_key(&ruleset.entry) {
        errors.push(GraphError {
            graph: ruleset.entry.clone(),
            error: Error::UnknownGraph(ruleset.entry.clone()),
        });
    }
    for (name, graph) in &ruleset.graphs {
        let mut found = check(registry, ruleset, graph);
        if runs_itself(ruleset, name, &mut Vec::new()) {
            found.push(Error::RunsItself);
        }
        errors.extend(found.into_iter().map(|error| GraphError {
            graph: name.clone(),
            error,
        }));
    }
    errors
}

fn check(
    registry: &Registry,
    ruleset: &Ruleset,
    graph: &Graph,
) -> Vec<Error> {
    let mut errors = Vec::new();
    let mut ids = BTreeSet::new();
    let mut ports_of = BTreeMap::new();
    for node in &graph.nodes {
        if !ids.insert(node.id) {
            errors.push(Error::SameId(node.id));
        }
        match ports(registry, ruleset, graph, node) {
            Ok(found) => {
                ports_of.insert(node.id, found);
            }
            Err(error) => errors.push(error),
        }
    }
    let port_type = |node: NodeId, name: &str, output: bool| {
        ports_of.get(&node).and_then(|(inputs, outputs)| {
            let list = if output { outputs } else { inputs };
            list.iter()
                .find(|port| port.name == name)
                .map(|port| port.value_type)
        })
    };
    for node in &graph.nodes {
        let choices = match &node.does {
            Does::Kind(name) => {
                registry.get(name).map(|kind| kind.inputs)
            }
            Does::Graph(_) | Does::Input(_) => None,
        };
        for (port, setting) in &node.settings {
            let Some(expected) = port_type(node.id, port, false)
            else {
                if ports_of.contains_key(&node.id) {
                    errors.push(Error::UnknownPort {
                        node: node.id,
                        port: port.clone(),
                    });
                }
                continue;
            };
            let found = match setting {
                Setting::Fixed(constant) => {
                    let allowed = choices
                        .and_then(|inputs| {
                            inputs
                                .iter()
                                .find(|spec| spec.is_for(port))
                        })
                        .map_or(&[][..], |spec| spec.choices);
                    if let Constant::Choice(choice) = constant
                        && !allowed.is_empty()
                        && !allowed.contains(&choice.as_str())
                    {
                        errors.push(Error::Invalid {
                            node: node.id,
                            port: port.clone(),
                            reason: "is not one of its choices",
                        });
                    }
                    constant.value_type()
                }
                Setting::Random(roll) => roll.spread.value_type(),
            };
            if !expected.accepts(found) {
                errors.push(Error::WrongType {
                    node: node.id,
                    port: port.clone(),
                    expected,
                    found,
                });
            }
        }
    }
    let mut wired = BTreeSet::new();
    for wire in &graph.wires {
        for end in [&wire.from, &wire.to] {
            if !ids.contains(&end.node) {
                errors.push(Error::UnknownNode(end.node));
            }
        }
        let from = port_type(wire.from.node, &wire.from.name, true);
        let to = port_type(wire.to.node, &wire.to.name, false);
        for (end, found) in [(&wire.from, from), (&wire.to, to)] {
            if found.is_none() && ports_of.contains_key(&end.node) {
                errors.push(Error::UnknownPort {
                    node: end.node,
                    port: end.name.clone(),
                });
            }
        }
        if let (Some(from), Some(to)) = (from, to)
            && !to.accepts(from)
        {
            errors.push(Error::WrongType {
                node: wire.to.node,
                port: wire.to.name.clone(),
                expected: to,
                found: from,
            });
        }
        if !wired.insert((wire.to.node, wire.to.name.as_str())) {
            errors.push(Error::TwoWires {
                node: wire.to.node,
                port: wire.to.name.clone(),
            });
        }
    }
    for output in &graph.outputs {
        match port_type(output.from.node, &output.from.name, true) {
            Some(found) if !output.value_type.accepts(found) => {
                errors.push(Error::WrongType {
                    node: output.from.node,
                    port: output.from.name.clone(),
                    expected: output.value_type,
                    found,
                });
            }
            Some(_) => {}
            None => errors.push(Error::UnknownPort {
                node: output.from.node,
                port: output.from.name.clone(),
            }),
        }
    }
    if let Err(error) = order(graph) {
        errors.push(error);
    }
    errors
}

fn runs_itself(
    ruleset: &Ruleset,
    name: &str,
    running: &mut Vec<String>,
) -> bool {
    if running.iter().any(|above| above == name) {
        return true;
    }
    let Some(graph) = ruleset.graphs.get(name) else {
        return false;
    };
    running.push(name.to_owned());
    let found = graph.nodes.iter().any(|node| match &node.does {
        Does::Graph(child) => runs_itself(ruleset, child, running),
        Does::Kind(_) | Does::Input(_) => false,
    });
    running.pop();
    found
}

/// The nodes in an order where each comes after those wired into it.
pub fn order(graph: &Graph) -> Result<Vec<usize>, Error> {
    let index = graph
        .nodes
        .iter()
        .enumerate()
        .map(|(at, node)| (node.id, at))
        .collect::<BTreeMap<_, _>>();
    let mut waiting = vec![0usize; graph.nodes.len()];
    for wire in &graph.wires {
        if let (Some(_), Some(&to)) =
            (index.get(&wire.from.node), index.get(&wire.to.node))
        {
            waiting[to] += 1;
        }
    }
    let mut ready = (0..graph.nodes.len())
        .filter(|&at| waiting[at] == 0)
        .collect::<Vec<_>>();
    let mut sorted = Vec::with_capacity(graph.nodes.len());
    while let Some(at) = ready.pop() {
        sorted.push(at);
        // Of nodes sharing an id, only the one `index` holds has its
        // wires counted, so only it frees what they lead to.
        if index.get(&graph.nodes[at].id) != Some(&at) {
            continue;
        }
        for wire in graph
            .wires
            .iter()
            .filter(|wire| wire.from.node == graph.nodes[at].id)
        {
            if let Some(&to) = index.get(&wire.to.node) {
                waiting[to] -= 1;
                if waiting[to] == 0 {
                    ready.push(to);
                }
            }
        }
    }
    if sorted.len() < graph.nodes.len() {
        let stuck = (0..graph.nodes.len())
            .filter(|&at| waiting[at] > 0)
            .map(|at| graph.nodes[at].id)
            .collect();
        return Err(Error::Loop(stuck));
    }
    Ok(sorted)
}
