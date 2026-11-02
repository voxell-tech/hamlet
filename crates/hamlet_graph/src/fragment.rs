//! Nodes lifted out of a sheet, to put into one again.

use std::collections::{BTreeMap, BTreeSet};

use bevy_math::Vec2;
use serde::{Deserialize, Serialize};

use crate::edit::Edit;
use crate::graph::{Node, NodeId, Sheet, Wire};

/// Nodes copied from a [`Sheet`], with where they were drawn and the
/// wires among them.
#[derive(
    Clone, PartialEq, Debug, Default, Serialize, Deserialize,
)]
pub struct Fragment {
    pub nodes: Vec<Node>,
    pub layout: BTreeMap<NodeId, Vec2>,
    pub wires: Vec<Wire>,
}

impl Fragment {
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// The top left corner of the box the drawn nodes' places span.
    pub fn corner(&self) -> Option<Vec2> {
        self.layout.values().copied().reduce(Vec2::min)
    }

    /// Drops every node `keep` refuses, and the wires to them.
    pub fn retain(&mut self, keep: impl Fn(&Node) -> bool) {
        self.nodes.retain(|node| keep(node));
        let kept = self
            .nodes
            .iter()
            .map(|node| node.id)
            .collect::<BTreeSet<_>>();
        self.layout.retain(|id, _| kept.contains(id));
        self.wires.retain(|wire| {
            kept.contains(&wire.from.node)
                && kept.contains(&wire.to.node)
        });
    }
}

impl Sheet {
    /// The nodes of `nodes` that are in the sheet, with the wires
    /// between them.
    pub fn copy(&self, nodes: &BTreeSet<NodeId>) -> Fragment {
        let graph = &self.graph;
        let found = graph
            .nodes
            .iter()
            .filter(|node| nodes.contains(&node.id))
            .cloned()
            .collect::<Vec<_>>();
        let ids =
            found.iter().map(|node| node.id).collect::<Vec<_>>();
        let layout = ids
            .iter()
            .filter_map(|id| Some((*id, *self.layout.get(id)?)))
            .collect();
        let wires = graph
            .wires
            .iter()
            .filter(|wire| {
                ids.contains(&wire.from.node)
                    && ids.contains(&wire.to.node)
            })
            .cloned()
            .collect();
        Fragment {
            nodes: found,
            layout,
            wires,
        }
    }

    /// The edit that adds `fragment` as new nodes, drawn `by` further
    /// than they were, and the ids they get.
    pub fn paste(
        &self,
        fragment: &Fragment,
        by: Vec2,
    ) -> (Edit, Vec<NodeId>) {
        let mut next = self.graph.next_id();
        let mut fresh = BTreeMap::new();
        let mut edits = Vec::new();
        for node in &fragment.nodes {
            let mut node = node.clone();
            let at = fragment
                .layout
                .get(&node.id)
                .copied()
                .unwrap_or_default();
            fresh.insert(node.id, next);
            node.id = next;
            edits.push(Edit::AddNode {
                node,
                at: Some(at + by),
            });
            next = NodeId(next.0.saturating_add(1));
        }
        edits.extend(fragment.wires.iter().filter_map(|wire| {
            let mut wire = wire.clone();
            wire.from.node = *fresh.get(&wire.from.node)?;
            wire.to.node = *fresh.get(&wire.to.node)?;
            Some(Edit::Connect(wire))
        }));
        (Edit::Batch(edits), fresh.into_values().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Does, Port};

    fn node(id: u32) -> Node {
        Node {
            id: NodeId(id),
            does: Does::Kind("math".to_owned()),
            settings: BTreeMap::new(),
        }
    }

    fn wire(from: u32, to: u32) -> Wire {
        let port = |id, name: &str| Port {
            node: NodeId(id),
            name: name.to_owned(),
        };
        Wire {
            from: port(from, "out"),
            to: port(to, "in"),
        }
    }

    #[test]
    fn pasting_gives_fresh_ids_and_keeps_only_inner_wires() {
        let mut sheet = Sheet::default();
        sheet.graph.nodes = vec![node(1), node(2), node(3)];
        sheet.graph.wires = vec![wire(1, 2), wire(2, 3)];
        sheet.layout = (1..=3)
            .map(|id| (NodeId(id), Vec2::splat(id as f32)))
            .collect();

        let copied =
            sheet.copy(&BTreeSet::from([NodeId(1), NodeId(2)]));
        assert_eq!(copied.wires, vec![wire(1, 2)]);

        let (edit, ids) = sheet.paste(&copied, Vec2::new(10.0, 0.0));
        assert_eq!(ids, vec![NodeId(4), NodeId(5)]);
        sheet.apply(edit).unwrap();
        assert_eq!(sheet.graph.wires.len(), 3);
        assert!(sheet.graph.wires.contains(&wire(4, 5)));
        assert_eq!(sheet.layout[&NodeId(5)], Vec2::new(12.0, 2.0));
    }
}
