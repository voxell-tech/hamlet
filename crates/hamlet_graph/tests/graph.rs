use std::collections::{BTreeMap, BTreeSet};

use bevy_math::Vec2;
use hamlet_graph::{
    Does, Edit, Graph, Group, Node, NodeId, Port, Registry, Ruleset,
    Sheet, Wire, order, validate,
};

/// One box whose height is the graph's input `rise`.
const RISE: &str = r#"(
    entry: "tower",
    graphs: {"tower": (
        inputs: [(name: "rise", value_type: Number, default: Some(Number(5.0)),
                  vary: Some((low: 1.0, high: 2.0, per: Item)))],
        outputs: [(name: "objects", value_type: Objects, from: (node: 3, name: "objects"))],
        nodes: [
            (id: 1, does: Kind("point"), settings: {"x": Fixed(Number(1.0)), "z": Fixed(Number(1.0))}),
            (id: 2, does: Kind("transform")),
            (id: 3, does: Kind("object")),
            (id: 4, does: Input("rise")),
        ],
        wires: [
            (from: (node: 4, name: "value"), to: (node: 1, name: "y")),
            (from: (node: 1, name: "point"), to: (node: 2, name: "scale")),
            (from: (node: 2, name: "transform"), to: (node: 3, name: "transform")),
        ],
    )},
)"#;

fn rise() -> Ruleset {
    Ruleset::from_ron(RISE).expect("the fixture parses")
}

#[test]
fn renaming_an_input_carries_to_the_nodes_that_give_it_and_undoes() {
    let mut sheet = Sheet {
        graph: rise().graphs["tower"].clone(),
        ..Sheet::default()
    };
    let mut renamed = sheet.graph.inputs[0].clone();
    renamed.name = "height".to_owned();
    let undo = sheet
        .apply(Edit::ChangeInput {
            name: "rise".to_owned(),
            input: renamed,
        })
        .expect("the input is there");
    let giving = |sheet: &Sheet| {
        sheet.graph.node(NodeId(4)).expect("node 4").does.clone()
    };
    assert_eq!(giving(&sheet), Does::Input("height".to_owned()));
    sheet.apply(undo).expect("the undo applies");
    assert_eq!(giving(&sheet), Does::Input("rise".to_owned()));
    assert_eq!(sheet.graph.inputs[0].name, "rise");
}

#[test]
fn removing_an_input_removes_the_nodes_that_give_it_and_undoes() {
    let mut sheet = Sheet {
        graph: rise().graphs["tower"].clone(),
        ..Sheet::default()
    };
    let before = sheet.clone();
    let undo = sheet
        .apply(Edit::RemoveInput("rise".to_owned()))
        .expect("the input is there");
    assert!(sheet.graph.node(NodeId(4)).is_none());
    assert!(sheet.graph.inputs.is_empty());
    assert_eq!(sheet.graph.wires.len(), 2);
    sheet.apply(undo).expect("the undo applies");
    assert_eq!(sheet, before);
}

#[test]
fn sheet_round_trips_and_edits_undo() {
    let mut sheet = Sheet {
        graph: rise().graphs["tower"].clone(),
        layout: BTreeMap::from([(NodeId(1), Vec2::new(10.0, 20.0))]),
        outputs_at: Some(Vec2::new(500.0, 20.0)),
        groups: vec![Group {
            title: "posts".to_owned(),
            nodes: BTreeSet::from([NodeId(1), NodeId(2)]),
        }],
    };
    let text = sheet.to_ron().expect("the sheet writes");
    assert_eq!(
        Sheet::from_ron(&text).expect("the sheet reads"),
        sheet
    );

    let before = sheet.clone();
    let undo = sheet
        .apply(Edit::RemoveNode(NodeId(2)))
        .expect("the node is there");
    assert!(sheet.graph.wires.len() < before.graph.wires.len());
    assert_eq!(sheet.groups[0].nodes, BTreeSet::from([NodeId(1)]));
    sheet.apply(undo).expect("the undo applies");
    assert_eq!(sheet.graph.nodes.len(), before.graph.nodes.len());
    assert_eq!(sheet.graph.wires.len(), before.graph.wires.len());
    assert_eq!(sheet.layout, before.layout);
    assert_eq!(sheet.groups, before.groups);
}

#[test]
fn a_graph_of_kinds_no_registry_has_does_not_validate() {
    let errors = validate(&Registry::default(), &rise());
    assert!(errors.iter().any(|found| matches!(
        found.error,
        hamlet_graph::Error::UnknownKind { .. }
    )));
}

#[test]
fn nodes_sharing_an_id_order_without_panicking() {
    let node = |id| Node {
        id: NodeId(id),
        does: Does::Kind("any".to_owned()),
        settings: BTreeMap::new(),
    };
    let port = |id| Port {
        node: NodeId(id),
        name: "x".to_owned(),
    };
    let graph = Graph {
        nodes: vec![node(1), node(1), node(2)],
        wires: vec![Wire {
            from: port(1),
            to: port(2),
        }],
        ..Graph::default()
    };
    assert_eq!(order(&graph).expect("no loop").len(), 3);
}
