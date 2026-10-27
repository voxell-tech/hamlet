use std::collections::BTreeMap;

use bevy_math::{Vec2, Vec3};
use hamlet_blueprint::{
    Does, Edit, Instance, Node, NodeId, Problem, Registry, Ruleset,
    Seed, Seeds, Setting, Sheet, Value, evaluate, validate,
};

/// Posts along a line, each lifted and turned at random, per
/// placement.
const FENCE: &str = r#"(
    entry: "fence",
    graphs: {
        "fence": (
            inputs: [(name: "end", value_type: Point, default: Some(Point((10.0, 0.0, 0.0))))],
            outputs: [(name: "geometry", value_type: Geometry, from: (node: 4, name: "geometry"))],
            nodes: [
                (id: 1, does: Kind("line"), settings: {"to": FromParent("end")}),
                (id: 2, does: Kind("along_curve"), settings: {"spacing": Fixed(Number(1.0))}),
                (id: 3, does: Kind("transform"), settings: {
                    "move_y": Random((spread: Range(0.0, 0.5), scope: Placement)),
                    "yaw": Random((spread: Range(-10.0, 10.0), scope: Placement)),
                }),
                (id: 4, does: Kind("instance"), settings: {"asset": Fixed(Choice("post"))}),
            ],
            wires: [
                (from: (node: 1, name: "curve"), to: (node: 2, name: "curve")),
                (from: (node: 2, name: "placements"), to: (node: 3, name: "placements")),
                (from: (node: 3, name: "placements"), to: (node: 4, name: "placements")),
            ],
        ),
        "yard": (
            outputs: [
                (name: "near", value_type: Geometry, from: (node: 1, name: "geometry")),
                (name: "far", value_type: Geometry, from: (node: 2, name: "geometry")),
            ],
            nodes: [
                (id: 1, does: Graph("fence")),
                (id: 2, does: Graph("fence")),
            ],
        ),
    },
)"#;

fn fence() -> Ruleset {
    Ruleset::from_ron(FENCE).expect("the fixture parses")
}

fn seeds(house: u64) -> Seeds {
    Seeds {
        realm: Seed(1),
        hamlet: Seed(2),
        house: Seed(house),
    }
}

fn geometry(
    outputs: &BTreeMap<String, Value>,
    name: &str,
) -> Vec<Instance> {
    match &outputs[name] {
        Value::Geometry(instances) => instances.clone(),
        other => panic!("{name} is {other:?}"),
    }
}

fn run(
    ruleset: &Ruleset,
    house: u64,
    inputs: &BTreeMap<String, Value>,
) -> BTreeMap<String, Value> {
    evaluate(&Registry::builtin(), ruleset, seeds(house), inputs)
        .expect("the ruleset runs")
}

#[test]
fn longer_curve_keeps_earlier_posts_and_their_rolls() {
    let ruleset = fence();
    let short =
        geometry(&run(&ruleset, 7, &BTreeMap::new()), "geometry");
    let long_end = BTreeMap::from([(
        "end".to_owned(),
        Value::Point(Vec3::new(20.0, 0.0, 0.0)),
    )]);
    let long = geometry(&run(&ruleset, 7, &long_end), "geometry");
    assert_eq!(short.len(), 11);
    assert_eq!(long.len(), 21);
    assert_eq!(short, long[..11]);

    let other_house =
        geometry(&run(&ruleset, 8, &BTreeMap::new()), "geometry");
    assert_ne!(short, other_house);
}

#[test]
fn a_graph_run_twice_differs_and_an_added_node_changes_nothing() {
    let mut ruleset = fence();
    ruleset.entry = "yard".to_owned();
    let before = run(&ruleset, 7, &BTreeMap::new());
    assert_ne!(geometry(&before, "near"), geometry(&before, "far"));

    let yard =
        ruleset.graphs.get_mut("yard").expect("the yard graph");
    yard.nodes.insert(
        0,
        Node {
            id: NodeId(9),
            does: Does::Kind("grid".to_owned()),
            settings: BTreeMap::new(),
        },
    );
    assert_eq!(run(&ruleset, 7, &BTreeMap::new()), before);
}

#[test]
fn closed_rectangle_places_on_its_corners() {
    let ruleset = Ruleset::from_ron(
        r#"(
            entry: "ring",
            graphs: {"ring": (
                outputs: [(name: "placements", value_type: Placements, from: (node: 2, name: "placements"))],
                nodes: [
                    (id: 1, does: Kind("rectangle")),
                    (id: 2, does: Kind("along_curve"), settings: {"spacing": Fixed(Number(2.0))}),
                ],
                wires: [(from: (node: 1, name: "curve"), to: (node: 2, name: "curve"))],
            )},
        )"#,
    )
    .expect("the fixture parses");
    let outputs = run(&ruleset, 1, &BTreeMap::new());
    let Value::Placements(placements) = &outputs["placements"] else {
        panic!("not placements");
    };
    // A 6 by 4 rectangle is 20 round: ten posts, none doubled at the
    // start.
    assert_eq!(placements.len(), 10);
    assert!(
        placements[3]
            .frame
            .translation
            .abs_diff_eq(Vec3::new(3.0, 0.0, -2.0), 1e-5)
    );
    assert!(
        placements[8]
            .frame
            .translation
            .abs_diff_eq(Vec3::new(-3.0, 0.0, 2.0), 1e-5)
    );
    // On a corner a post faces along the edge that starts there.
    let facing = placements[3].frame.rotation * Vec3::X;
    assert!(facing.abs_diff_eq(Vec3::Z, 1e-5));
    assert!((placements[5].along - 0.5).abs() < 1e-6);
}

#[test]
fn validation_finds_a_wrong_wire_a_loop_and_a_graph_that_runs_itself()
{
    let ruleset = Ruleset::from_ron(
        r#"(
            entry: "bad",
            graphs: {"bad": (
                nodes: [
                    (id: 1, does: Kind("line")),
                    (id: 2, does: Kind("cull")),
                    (id: 3, does: Kind("repeat")),
                    (id: 4, does: Graph("bad")),
                ],
                wires: [
                    (from: (node: 1, name: "curve"), to: (node: 2, name: "placements")),
                    (from: (node: 2, name: "placements"), to: (node: 3, name: "placements")),
                    (from: (node: 3, name: "placements"), to: (node: 2, name: "keep")),
                ],
            )},
        )"#,
    )
    .expect("the fixture parses");
    let problems = validate(&Registry::builtin(), &ruleset)
        .into_iter()
        .map(|found| found.problem)
        .collect::<Vec<_>>();
    assert!(problems.iter().any(|problem| matches!(
        problem,
        Problem::WrongType { node: NodeId(2), port, .. } if port == "placements"
    )));
    assert!(
        problems
            .iter()
            .any(|problem| matches!(problem, Problem::Loop(_)))
    );
    assert!(problems.contains(&Problem::RunsItself));
}

#[test]
fn math_on_a_random_field_is_worked_out_at_each_placement() {
    let ruleset = Ruleset::from_ron(
        r#"(
            entry: "thin",
            graphs: {"thin": (
                outputs: [(name: "placements", value_type: Placements, from: (node: 5, name: "placements"))],
                nodes: [
                    (id: 1, does: Kind("line"), settings: {"to": Fixed(Point((99.0, 0.0, 0.0)))}),
                    (id: 2, does: Kind("along_curve")),
                    (id: 3, does: Kind("random")),
                    (id: 4, does: Kind("math"), settings: {"op": Fixed(Choice("less")), "b": Fixed(Number(0.5))}),
                    (id: 5, does: Kind("cull")),
                ],
                wires: [
                    (from: (node: 1, name: "curve"), to: (node: 2, name: "curve")),
                    (from: (node: 3, name: "field"), to: (node: 4, name: "a")),
                    (from: (node: 2, name: "placements"), to: (node: 5, name: "placements")),
                    (from: (node: 4, name: "value"), to: (node: 5, name: "keep")),
                ],
            )},
        )"#,
    )
    .expect("the fixture parses");
    let outputs = run(&ruleset, 1, &BTreeMap::new());
    let Value::Placements(kept) = &outputs["placements"] else {
        panic!("not placements");
    };
    // Rolled once for all, all 100 would stay or go together.
    assert!((25..75).contains(&kept.len()), "{} kept", kept.len());
}

/// One box whose height is the graph's input `rise`, which varies
/// between 1 and 2 for each house when nothing sets it.
const RISE: &str = r#"(
    entry: "tower",
    graphs: {"tower": (
        inputs: [(name: "rise", value_type: Number, default: Some(Number(5.0)),
                  vary: Some((low: 1.0, high: 2.0, per: House)))],
        outputs: [(name: "geometry", value_type: Geometry, from: (node: 3, name: "geometry"))],
        nodes: [
            (id: 1, does: Kind("point"), settings: {"x": Fixed(Number(1.0)), "y": FromParent("rise"), "z": Fixed(Number(1.0))}),
            (id: 2, does: Kind("box")),
            (id: 3, does: Kind("instance")),
        ],
        wires: [
            (from: (node: 1, name: "point"), to: (node: 2, name: "size")),
            (from: (node: 2, name: "placements"), to: (node: 3, name: "placements")),
        ],
    )},
)"#;

#[test]
fn an_unset_input_varies_within_its_range_once_per_house() {
    let ruleset =
        Ruleset::from_ron(RISE).expect("the fixture parses");
    let height = |house| {
        geometry(&run(&ruleset, house, &BTreeMap::new()), "geometry")
            [0]
        .size
        .y
    };
    let heights = (1..20).map(height).collect::<Vec<_>>();
    assert!(
        heights.iter().all(|h| (1.0..2.0).contains(h)),
        "{heights:?}"
    );
    assert!(heights.windows(2).any(|pair| pair[0] != pair[1]));
    assert_eq!(height(7), height(7));
    // Set from above, it is what it is set to.
    let given =
        BTreeMap::from([("rise".to_owned(), Value::Number(3.0))]);
    assert_eq!(
        geometry(&run(&ruleset, 7, &given), "geometry")[0].size.y,
        3.0
    );
}

#[test]
fn renaming_an_input_carries_to_the_nodes_set_from_it_and_undoes() {
    let ruleset =
        Ruleset::from_ron(RISE).expect("the fixture parses");
    let mut sheet = Sheet {
        graph: ruleset.graphs["tower"].clone(),
        layout: BTreeMap::new(),
    };
    let mut renamed = sheet.graph.inputs[0].clone();
    renamed.name = "height".to_owned();
    let undo = sheet
        .apply(Edit::ChangeInput {
            name: "rise".to_owned(),
            input: renamed,
        })
        .expect("the input is there");
    let read_by = |sheet: &Sheet| {
        sheet.graph.node(NodeId(1)).expect("node 1").settings["y"]
            .clone()
    };
    assert_eq!(
        read_by(&sheet),
        Setting::FromParent("height".to_owned())
    );
    sheet.apply(undo).expect("the undo applies");
    assert_eq!(
        read_by(&sheet),
        Setting::FromParent("rise".to_owned())
    );
    assert_eq!(sheet.graph.inputs[0].name, "rise");
}

#[test]
fn sheet_round_trips_and_edits_undo() {
    let mut sheet = Sheet {
        graph: fence().graphs["fence"].clone(),
        layout: BTreeMap::from([(NodeId(1), Vec2::new(10.0, 20.0))]),
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
    sheet.apply(undo).expect("the undo applies");
    assert_eq!(sheet.graph.nodes.len(), before.graph.nodes.len());
    assert_eq!(sheet.graph.wires.len(), before.graph.wires.len());
    assert_eq!(sheet.layout, before.layout);
}
