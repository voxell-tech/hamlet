//! Graphs as older files wrote them. A parameter was once set on the
//! node that used it, and is now a node of its own wired to each
//! node that uses it; reading a file makes that change, so nothing
//! else sees the old form.

use std::collections::BTreeMap;

use bevy_math::Vec2;
use serde::Deserialize;

use crate::graph::{
    Does, Graph, GraphInput, GraphOutput, Group, Node, NodeId, Port,
    Roll, Setting, Sheet, Wire,
};
use crate::value::Constant;

/// How far left of a node its parameter node is drawn, in sheet
/// pixels.
pub const PARAMETER_GAP: f32 = 260.0;
/// How far down a parameter is moved off another drawn at its place.
const BELOW: f32 = 80.0;

/// A [`Graph`] as a file holds it.
#[derive(Deserialize)]
pub(crate) struct GraphFile {
    #[serde(default)]
    inputs: Vec<GraphInput>,
    #[serde(default)]
    outputs: Vec<GraphOutput>,
    nodes: Vec<NodeFile>,
    #[serde(default)]
    wires: Vec<Wire>,
}

#[derive(Deserialize)]
struct NodeFile {
    id: NodeId,
    does: Does,
    #[serde(default)]
    settings: BTreeMap<String, SettingFile>,
}

/// A [`Setting`], or the `FromParent` that set an input from one of
/// the graph's own.
#[derive(Deserialize)]
enum SettingFile {
    Fixed(Constant),
    Random(Roll),
    FromParent(String),
}

/// A [`Sheet`] as a file holds it.
#[derive(Deserialize)]
pub(crate) struct SheetFile {
    graph: GraphFile,
    #[serde(default)]
    layout: BTreeMap<NodeId, Vec2>,
    #[serde(default)]
    outputs_at: Option<Vec2>,
    #[serde(default)]
    groups: Vec<Group>,
}

impl GraphFile {
    /// The graph, and each parameter node made with the node that
    /// first used it. One parameter node is made for each name, and
    /// wired to every input that was set from it. A setting from a
    /// name the graph has no input of, or on an input that has a
    /// wire, set nothing and is dropped.
    fn lift(self) -> (Graph, Vec<(NodeId, NodeId)>) {
        let Self {
            inputs,
            outputs,
            nodes,
            mut wires,
        } = self;
        let mut next = nodes
            .iter()
            .map(|node| node.id.0.saturating_add(1))
            .max()
            .unwrap_or(1);
        let mut made = BTreeMap::<String, NodeId>::new();
        let mut firsts = Vec::new();
        let mut parameters = Vec::new();
        let mut joined = Vec::new();
        let mut kept = Vec::with_capacity(nodes.len());
        for node in nodes {
            let mut settings = BTreeMap::new();
            for (port, setting) in node.settings {
                let setting = match setting {
                    SettingFile::Fixed(constant) => {
                        Setting::Fixed(constant)
                    }
                    SettingFile::Random(roll) => {
                        Setting::Random(roll)
                    }
                    SettingFile::FromParent(name) => {
                        let wired = wires.iter().any(|wire| {
                            wire.to.node == node.id
                                && wire.to.name == port
                        });
                        let known = inputs
                            .iter()
                            .any(|input| input.name == name);
                        if wired || !known {
                            continue;
                        }
                        let from = *made
                            .entry(name.clone())
                            .or_insert_with(|| {
                                let id = NodeId(next);
                                next = next.saturating_add(1);
                                firsts.push((id, node.id));
                                parameters.push(Node {
                                    id,
                                    does: Does::Input(name),
                                    settings: BTreeMap::new(),
                                });
                                id
                            });
                        joined.push(Wire {
                            from: Port {
                                node: from,
                                name: Does::INPUT_PORT.to_owned(),
                            },
                            to: Port {
                                node: node.id,
                                name: port,
                            },
                        });
                        continue;
                    }
                };
                settings.insert(port, setting);
            }
            kept.push(Node {
                id: node.id,
                does: node.does,
                settings,
            });
        }
        kept.extend(parameters);
        wires.extend(joined);
        let graph = Graph {
            inputs,
            outputs,
            nodes: kept,
            wires,
        };
        (graph, firsts)
    }
}

impl From<GraphFile> for Graph {
    fn from(file: GraphFile) -> Self {
        file.lift().0
    }
}

impl From<SheetFile> for Sheet {
    fn from(file: SheetFile) -> Self {
        let SheetFile {
            graph,
            mut layout,
            outputs_at,
            mut groups,
        } = file;
        let (graph, firsts) = graph.lift();
        let mut placed = Vec::<Vec2>::new();
        for (parameter, user) in firsts {
            let Some(&user_at) = layout.get(&user) else {
                continue;
            };
            let mut at = user_at - Vec2::new(PARAMETER_GAP, 0.0);
            while placed.iter().any(|other| {
                (other.x - at.x).abs() < 1.0
                    && (other.y - at.y).abs() < BELOW
            }) {
                at.y += BELOW;
            }
            placed.push(at);
            layout.insert(parameter, at);
            for group in &mut groups {
                if group.nodes.contains(&user) {
                    group.nodes.insert(parameter);
                }
            }
        }
        Sheet {
            graph,
            layout,
            outputs_at,
            groups,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OLD: &str = r#"(
        graph: (
            inputs: [(name: "rise", value_type: Number, default: Some(Number(5.0)))],
            nodes: [
                (id: 1, does: Kind("point"), settings: {"x": FromParent("rise"), "y": FromParent("rise"), "z": Fixed(Number(1.0))}),
                (id: 2, does: Kind("point"), settings: {"x": FromParent("rise"), "y": FromParent("gone")}),
            ],
        ),
        layout: {1: (300.0, 100.0), 2: (600.0, 100.0)},
    )"#;

    #[test]
    fn a_parameter_set_on_nodes_becomes_one_node_wired_to_each() {
        let sheet = ron::from_str::<Sheet>(OLD).unwrap();
        let graph = &sheet.graph;
        let parameters = graph
            .nodes
            .iter()
            .filter(|node| {
                node.does == Does::Input("rise".to_owned())
            })
            .collect::<Vec<_>>();
        assert_eq!(parameters.len(), 1);
        let id = parameters[0].id;
        // Node 2's `y` named an input the graph does not have.
        assert_eq!(graph.wires.len(), 3);
        assert!(graph.wires.iter().all(|wire| wire.from.node == id));
        assert_eq!(
            graph.node(NodeId(1)).unwrap().settings,
            BTreeMap::from([(
                "z".to_owned(),
                Setting::Fixed(Constant::Number(1.0))
            )])
        );
        assert!(graph.node(NodeId(2)).unwrap().settings.is_empty());
        assert_eq!(sheet.layout[&id], Vec2::new(40.0, 100.0));
    }
}
