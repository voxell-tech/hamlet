use std::collections::BTreeMap;

use bevy_math::Vec3;
use hamlet_blueprint::{
    Bundle, Object, Seed, Seeds, Value, evaluate, evaluate_partly,
    registry,
};
use hamlet_graph::{Does, Error, Node, NodeId, Ruleset, validate};

/// A post copied along a line, each copy lifted and turned at random
/// by a transform read from random fields at each.
const FENCE: &str = r#"(
    entry: "fence",
    graphs: {
        "fence": (
            inputs: [(name: "end", value_type: Point, default: Some(Point((10.0, 0.0, 0.0))))],
            outputs: [(name: "objects", value_type: Objects, from: (node: 5, name: "objects"))],
            nodes: [
                (id: 1, does: Kind("line"), settings: {"to": FromParent("end")}),
                (id: 2, does: Kind("along_curve"), settings: {"spacing": Fixed(Number(1.0))}),
                (id: 3, does: Kind("transform")),
                (id: 4, does: Kind("object"), settings: {"asset": Fixed(Choice("post"))}),
                (id: 5, does: Kind("matmul")),
                (id: 6, does: Kind("point")),
                (id: 7, does: Kind("point")),
                (id: 8, does: Kind("random"), settings: {"high": Fixed(Number(0.5))}),
                (id: 9, does: Kind("random"), settings: {"low": Fixed(Number(-10.0)), "high": Fixed(Number(10.0))}),
            ],
            wires: [
                (from: (node: 1, name: "curve"), to: (node: 2, name: "curve")),
                (from: (node: 4, name: "objects"), to: (node: 2, name: "objects")),
                (from: (node: 8, name: "field"), to: (node: 6, name: "y")),
                (from: (node: 9, name: "field"), to: (node: 7, name: "y")),
                (from: (node: 6, name: "point"), to: (node: 3, name: "translation")),
                (from: (node: 7, name: "point"), to: (node: 3, name: "rotation")),
                (from: (node: 2, name: "objects"), to: (node: 5, name: "objects")),
                (from: (node: 3, name: "transform"), to: (node: 5, name: "transform")),
            ],
        ),
        "yard": (
            outputs: [
                (name: "near", value_type: Objects, from: (node: 1, name: "objects")),
                (name: "far", value_type: Objects, from: (node: 2, name: "objects")),
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
        item: Seed(house),
    }
}

fn objects(
    outputs: &BTreeMap<String, Value>,
    name: &str,
) -> Vec<Object> {
    match &outputs[name] {
        Value::Objects(objects) => objects.clone(),
        other => panic!("{name} is {other:?}"),
    }
}

fn run(
    ruleset: &Ruleset,
    house: u64,
    inputs: &BTreeMap<String, Value>,
) -> BTreeMap<String, Value> {
    evaluate(&registry(), ruleset, seeds(house), inputs)
        .expect("the ruleset runs")
}

#[test]
fn longer_curve_keeps_earlier_posts_and_their_rolls() {
    let ruleset = fence();
    let short =
        objects(&run(&ruleset, 7, &BTreeMap::new()), "objects");
    let long_end = BTreeMap::from([(
        "end".to_owned(),
        Value::Point(Vec3::new(20.0, 0.0, 0.0)),
    )]);
    let long = objects(&run(&ruleset, 7, &long_end), "objects");
    assert_eq!(short.len(), 11);
    assert_eq!(long.len(), 21);
    // Where each post is and how it is turned hold; how far along the
    // fence it is does not.
    let placed = |objects: &[Object]| {
        objects
            .iter()
            .map(|object| (object.id, object.frame))
            .collect::<Vec<_>>()
    };
    assert_eq!(placed(&short), placed(&long[..11]));

    let other_house =
        objects(&run(&ruleset, 8, &BTreeMap::new()), "objects");
    assert_ne!(short, other_house);
}

#[test]
fn a_graph_run_twice_differs_and_an_added_node_changes_nothing() {
    let mut ruleset = fence();
    ruleset.entry = "yard".to_owned();
    let before = run(&ruleset, 7, &BTreeMap::new());
    assert_ne!(objects(&before, "near"), objects(&before, "far"));

    let yard =
        ruleset.graphs.get_mut("yard").expect("the yard graph");
    yard.nodes.insert(
        0,
        Node {
            id: NodeId(9),
            does: Does::Kind("polygon".to_owned()),
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
                outputs: [(name: "objects", value_type: Objects, from: (node: 2, name: "objects"))],
                nodes: [
                    (id: 1, does: Kind("rectangle")),
                    (id: 2, does: Kind("along_curve"), settings: {"spacing": Fixed(Number(2.0))}),
                    (id: 3, does: Kind("object")),
                ],
                wires: [
                    (from: (node: 1, name: "curve"), to: (node: 2, name: "curve")),
                    (from: (node: 3, name: "objects"), to: (node: 2, name: "objects")),
                ],
            )},
        )"#,
    )
    .expect("the fixture parses");
    let outputs = run(&ruleset, 1, &BTreeMap::new());
    let transforms = objects(&outputs, "objects");
    // A 6 by 4 rectangle is 20 round: ten posts, none doubled at the
    // start.
    assert_eq!(transforms.len(), 10);
    assert!(
        transforms[3]
            .frame
            .translation
            .abs_diff_eq(Vec3::new(3.0, 0.0, -2.0), 1e-5)
    );
    assert!(
        transforms[8]
            .frame
            .translation
            .abs_diff_eq(Vec3::new(-3.0, 0.0, 2.0), 1e-5)
    );
    // On a corner a post faces along the edge that starts there.
    let facing = transforms[3].frame.rotation * Vec3::X;
    assert!(facing.abs_diff_eq(Vec3::Z, 1e-5));
    assert!((transforms[5].along - 0.5).abs() < 1e-6);
}

#[test]
fn open_curve_keeps_its_end_post_when_spacing_rounds_past_it() {
    let ruleset = Ruleset::from_ron(
        r#"(
            entry: "line",
            graphs: {"line": (
                outputs: [(name: "objects", value_type: Objects, from: (node: 2, name: "objects"))],
                nodes: [
                    (id: 1, does: Kind("line"), settings: {"to": Fixed(Point((0.9999999, 0.0, 0.0)))}),
                    (id: 2, does: Kind("along_curve"), settings: {"spacing": Fixed(Number(0.1))}),
                    (id: 3, does: Kind("object")),
                ],
                wires: [
                    (from: (node: 1, name: "curve"), to: (node: 2, name: "curve")),
                    (from: (node: 3, name: "objects"), to: (node: 2, name: "objects")),
                ],
            )},
        )"#,
    )
    .expect("the fixture parses");
    let posts =
        objects(&run(&ruleset, 1, &BTreeMap::new()), "objects");
    assert_eq!(posts.len(), 11);
    assert!(posts.iter().all(|post| post.count == 11));
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
                    (from: (node: 1, name: "curve"), to: (node: 2, name: "objects")),
                    (from: (node: 2, name: "objects"), to: (node: 3, name: "objects")),
                    (from: (node: 3, name: "objects"), to: (node: 2, name: "keep")),
                ],
            )},
        )"#,
    )
    .expect("the fixture parses");
    let errors = validate(&registry(), &ruleset)
        .into_iter()
        .map(|found| found.error)
        .collect::<Vec<_>>();
    assert!(errors.iter().any(|error| matches!(
        error,
        Error::WrongType { node: NodeId(2), port, .. } if port == "objects"
    )));
    assert!(
        errors.iter().any(|error| matches!(error, Error::Loop(_)))
    );
    assert!(errors.contains(&Error::RunsItself));
}

#[test]
fn math_on_a_random_field_is_worked_out_at_each_object() {
    let ruleset = Ruleset::from_ron(
        r#"(
            entry: "thin",
            graphs: {"thin": (
                outputs: [(name: "objects", value_type: Objects, from: (node: 5, name: "objects"))],
                nodes: [
                    (id: 1, does: Kind("line"), settings: {"to": Fixed(Point((99.0, 0.0, 0.0)))}),
                    (id: 2, does: Kind("along_curve")),
                    (id: 3, does: Kind("random")),
                    (id: 4, does: Kind("math"), settings: {"op": Fixed(Choice("less")), "b": Fixed(Number(0.5))}),
                    (id: 5, does: Kind("cull")),
                    (id: 6, does: Kind("object")),
                ],
                wires: [
                    (from: (node: 1, name: "curve"), to: (node: 2, name: "curve")),
                    (from: (node: 6, name: "objects"), to: (node: 2, name: "objects")),
                    (from: (node: 3, name: "field"), to: (node: 4, name: "a")),
                    (from: (node: 2, name: "objects"), to: (node: 5, name: "objects")),
                    (from: (node: 4, name: "value"), to: (node: 5, name: "keep")),
                ],
            )},
        )"#,
    )
    .expect("the fixture parses");
    let outputs = run(&ruleset, 1, &BTreeMap::new());
    let kept = objects(&outputs, "objects");
    // Rolled once for all, all 100 would stay or go together.
    assert!((25..75).contains(&kept.len()), "{} kept", kept.len());
}

/// One box whose height is the graph's input `rise`, which varies
/// between 1 and 2 for each house when nothing sets it.
const RISE: &str = r#"(
    entry: "tower",
    graphs: {"tower": (
        inputs: [(name: "rise", value_type: Number, default: Some(Number(5.0)),
                  vary: Some((low: 1.0, high: 2.0, per: Item)))],
        outputs: [(name: "objects", value_type: Objects, from: (node: 3, name: "objects"))],
        nodes: [
            (id: 1, does: Kind("point"), settings: {"x": Fixed(Number(1.0)), "y": FromParent("rise"), "z": Fixed(Number(1.0))}),
            (id: 2, does: Kind("transform")),
            (id: 3, does: Kind("object")),
        ],
        wires: [
            (from: (node: 1, name: "point"), to: (node: 2, name: "scale")),
            (from: (node: 2, name: "transform"), to: (node: 3, name: "transform")),
        ],
    )},
)"#;

#[test]
fn an_unset_input_varies_within_its_range_once_per_house() {
    let ruleset =
        Ruleset::from_ron(RISE).expect("the fixture parses");
    let height = |house| {
        objects(&run(&ruleset, house, &BTreeMap::new()), "objects")[0]
            .frame
            .scale
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
        objects(&run(&ruleset, 7, &given), "objects")[0]
            .frame
            .scale
            .y,
        3.0
    );
}

#[test]
fn a_mesh_at_scale_one_is_its_own_size_and_its_socket_is_on_it() {
    let ruleset = Ruleset::from_ron(
        r#"(
            entry: "door",
            graphs: {"door": (
                outputs: [(name: "objects", value_type: Objects, from: (node: 3, name: "objects"))],
                nodes: [
                    (id: 1, does: Kind("object"), settings: {"asset": Fixed(Choice("door"))}),
                    (id: 2, does: Kind("object"), settings: {"asset": Fixed(Choice("box"))}),
                    (id: 3, does: Kind("on_sockets")),
                ],
                wires: [
                    (from: (node: 1, name: "objects"), to: (node: 3, name: "on")),
                    (from: (node: 2, name: "objects"), to: (node: 3, name: "objects")),
                ],
            )},
            assets: {"door": (
                shape: Mesh("assets/door.glb"),
                size: (1.2, 2.4, 0.2),
                sockets: {"lamp": (at: (0.5, 0.5, 0.0), turn: (0.0, 0.0, 0.0, 1.0))},
            )},
        )"#,
    )
    .expect("the fixture parses");
    let lamps =
        objects(&run(&ruleset, 1, &BTreeMap::new()), "objects");
    // At the top right of a door 1.2 wide and 2.4 tall standing on
    // the origin, not of a metre's box.
    assert_eq!(lamps.len(), 1);
    assert!(
        lamps[0]
            .frame
            .translation
            .abs_diff_eq(Vec3::new(0.6, 2.4, 0.0), 1e-5)
    );
}

#[test]
fn a_bundle_round_trips_with_its_files() {
    let bundle = Bundle {
        ruleset: fence(),
        files: BTreeMap::from([(
            "assets/lamp.glb".to_owned(),
            vec![0, 255, 10, 13],
        )]),
    };
    let text = bundle.to_ron().expect("the bundle writes");
    assert_eq!(
        Bundle::from_ron(&text).expect("the bundle reads"),
        bundle
    );
}

#[test]
fn a_node_left_unwired_stops_only_what_needs_it() {
    let ruleset = Ruleset::from_ron(
        r#"(
            entry: "half",
            graphs: {"half": (
                nodes: [
                    (id: 1, does: Kind("object")),
                    (id: 2, does: Kind("along_curve")),
                    (id: 3, does: Kind("cull")),
                    (id: 4, does: Kind("object"), settings: {"asset": Fixed(Choice("post"))}),
                ],
                outputs: [
                    (name: "copied", value_type: Objects, from: (node: 3, name: "objects")),
                    (name: "alone", value_type: Objects, from: (node: 4, name: "objects")),
                ],
                wires: [
                    (from: (node: 1, name: "objects"), to: (node: 2, name: "objects")),
                    (from: (node: 2, name: "objects"), to: (node: 3, name: "objects")),
                ],
            )},
        )"#,
    )
    .expect("the fixture parses");
    let (made, errors) = evaluate_partly(
        &registry(),
        &ruleset,
        seeds(1),
        &BTreeMap::new(),
    )
    .expect("it validates");
    // The curve is not wired, so along_curve fails, and cull after it
    // is skipped without an error of its own.
    assert_eq!(objects(&made, "alone").len(), 1);
    assert!(!made.contains_key("copied"));
    assert_eq!(errors.len(), 1);
    assert!(matches!(
        errors[0].error,
        Error::Unset {
            node: NodeId(2),
            ..
        }
    ));
}

#[test]
fn a_random_select_picks_one_wired_set_and_keeps_it_per_house() {
    let ruleset = Ruleset::from_ron(
        r#"(
            entry: "pick",
            graphs: {"pick": (
                outputs: [(name: "objects", value_type: Objects, from: (node: 4, name: "objects"))],
                nodes: [
                    (id: 1, does: Kind("object"), settings: {"asset": Fixed(Choice("post"))}),
                    (id: 2, does: Kind("object"), settings: {"asset": Fixed(Choice("door"))}),
                    (id: 4, does: Kind("random_select")),
                ],
                wires: [
                    (from: (node: 1, name: "objects"), to: (node: 4, name: "set_1")),
                    (from: (node: 2, name: "objects"), to: (node: 4, name: "set_2")),
                ],
            )},
        )"#,
    )
    .expect("the fixture parses");
    let picked = |house| {
        let found = objects(
            &run(&ruleset, house, &BTreeMap::new()),
            "objects",
        );
        assert_eq!(found.len(), 1);
        found[0].asset.clone()
    };
    let assets = (0..32)
        .map(picked)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        assets,
        ["door", "post"].map(str::to_owned).into_iter().collect()
    );
    assert_eq!(picked(5), picked(5));
}
