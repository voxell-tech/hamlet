//! Inputs that grow a port for each wire into them.

use crate::edit::Edit;
use crate::graph::{Does, Graph, NodeId};
use crate::node::{PortSpec, Registry};

/// The ports of the growing input `spec` of `node`: one up to the
/// highest numbered wire into it, then an unwired one, and two at
/// least.
pub(crate) fn ports_of(
    spec: &PortSpec,
    graph: &Graph,
    node: NodeId,
) -> Vec<String> {
    let highest = graph
        .wires
        .iter()
        .filter(|wire| wire.to.node == node)
        .filter_map(|wire| spec.number_of(&wire.to.name))
        .max()
        .unwrap_or(0);
    (1..=(highest + 1).max(2))
        .map(|number| spec.port(number))
        .collect()
}

/// The edit that numbers the wires into each growing input from one
/// with no gaps, in the order they were. None when they already are.
pub fn pack(registry: &Registry, graph: &Graph) -> Option<Edit> {
    let mut cut = Vec::new();
    let mut joined = Vec::new();
    for node in &graph.nodes {
        let Does::Kind(kind) = &node.does else {
            continue;
        };
        let Some(kind) = registry.get(kind) else {
            continue;
        };
        for spec in kind.inputs.iter().filter(|spec| spec.grows) {
            let mut wired = graph
                .wires
                .iter()
                .filter(|wire| wire.to.node == node.id)
                .filter_map(|wire| {
                    Some((spec.number_of(&wire.to.name)?, wire))
                })
                .collect::<Vec<_>>();
            wired.sort_by_key(|(number, _)| *number);
            for (at, (number, wire)) in wired.into_iter().enumerate()
            {
                if number == at + 1 {
                    continue;
                }
                let mut moved = wire.clone();
                moved.to.name = spec.port(at + 1);
                cut.push(Edit::Disconnect(wire.clone()));
                joined.push(Edit::Connect(moved));
            }
        }
    }
    // Every wire leaves before any comes back, so none lands on a
    // port another still holds.
    (!cut.is_empty()).then(|| {
        cut.extend(joined);
        Edit::Batch(cut)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Node, Port, Sheet, Wire};
    use crate::node::NodeKind;
    use crate::value::ValueType;

    const SETS: NodeKind = NodeKind {
        name: "sets",
        doc: "",
        inputs: &[
            PortSpec::new("set", ValueType::Objects, "").grows()
        ],
        outputs: &[],
    };

    fn wire(from: u32, to: &str) -> Wire {
        Wire {
            from: Port {
                node: NodeId(from),
                name: "objects".to_owned(),
            },
            to: Port {
                node: NodeId(9),
                name: to.to_owned(),
            },
        }
    }

    #[test]
    fn unplugging_a_middle_wire_closes_the_gap_and_undoes_exactly() {
        let mut registry = Registry::default();
        registry.add("Test", SETS);
        let mut sheet = Sheet::default();
        sheet.graph.nodes = [1, 2, 3, 9]
            .map(|id| Node {
                id: NodeId(id),
                does: Does::Kind("sets".to_owned()),
                settings: Default::default(),
            })
            .into();
        sheet.graph.wires = vec![
            wire(1, "set_1"),
            wire(2, "set_2"),
            wire(3, "set_3"),
        ];
        let before = sheet.clone();

        let cut =
            sheet.apply(Edit::Disconnect(wire(2, "set_2"))).unwrap();
        let packing = pack(&registry, &sheet.graph).expect("a gap");
        let unpack = sheet.apply(packing).unwrap();
        assert_eq!(
            sheet.graph.wires,
            vec![wire(1, "set_1"), wire(3, "set_2")]
        );
        assert_eq!(
            ports_of(&SETS.inputs[0], &sheet.graph, NodeId(9)),
            ["set_1", "set_2", "set_3"]
        );
        assert!(pack(&registry, &sheet.graph).is_none());

        // The same wires come back, though not in the same order.
        sheet.apply(Edit::Batch(vec![unpack, cut])).unwrap();
        assert_eq!(sheet.graph.wires.len(), before.graph.wires.len());
        assert!(
            before
                .graph
                .wires
                .iter()
                .all(|wire| sheet.graph.wires.contains(wire))
        );
    }
}
