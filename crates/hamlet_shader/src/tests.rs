use std::collections::BTreeMap;

use bevy_math::Vec4;
use hamlet_graph::{
    Constant, Does, Graph, Node, NodeId, Port, Setting, Wire,
};

use crate::compile::compile;
use crate::registry;
use crate::wrap::SPHERE;

/// A node of every kind, each math kind on vectors or colours too.
fn every_kind() -> Graph {
    let mut graph = Graph::default();
    for (at, (_, kind)) in registry().kinds().enumerate() {
        graph.nodes.push(Node {
            id: NodeId(at as u32 + 1),
            does: Does::Kind(kind.name.to_owned()),
            settings: BTreeMap::new(),
        });
    }
    let id = |name: &str| {
        graph
            .nodes
            .iter()
            .find(|node| node.does == Does::Kind(name.to_owned()))
            .map(|node| node.id)
            .unwrap()
    };
    let wire = |from: (&str, &str), to: (&str, &str)| Wire {
        from: Port {
            node: id(from.0),
            name: from.1.to_owned(),
        },
        to: Port {
            node: id(to.0),
            name: to.1.to_owned(),
        },
    };
    graph.wires = vec![
        wire(("uv", "uv"), ("math", "a")),
        wire(("position", "position"), ("math", "b")),
        wire(("noise", "value"), ("mix", "factor")),
        wire(("surface colour", "colour"), ("mix", "a")),
        wire(("voronoi", "colour"), ("mix", "b")),
        wire(("mix", "value"), ("blend", "base")),
        wire(("brick", "colour"), ("blend", "layer")),
        wire(("blend", "colour"), ("surface", "base colour")),
        wire(("fresnel", "value"), ("surface", "emission")),
        wire(("math", "value"), ("split", "vector")),
        wire(("split", "x"), ("ramp", "factor")),
    ];
    graph
}

#[test]
fn every_kind_compiles_to_valid_wgsl() {
    let compiled = compile(&every_kind()).unwrap();
    assert_eq!(compiled.errors, []);
    let code = compiled
        .code
        .replace("GROUP", "0")
        .replace("SLOTS_BINDING", "0");
    // The swatch's own body, with a uniform for the node shown.
    let code = format!(
        "{code}
@group(0) @binding(1) var<uniform> shown: vec4<u32>;
struct UiVertexOutput {{ @location(0) uv: vec2<f32> }}
@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {{
    let s = surface(SurfaceIn(in.uv, vec3<f32>(0.0), vec3<f32>(0.0, \
         1.0, 0.0), vec3<f32>(0.0, 0.0, 1.0), vec4<f32>(1.0), 0.0, \
         vec3<f32>(0.0), vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(0.0, 0.0, \
         1.0)));
    if s.unlit > 2.0 {{ return s.base_color; }}
    {SPHERE}
}}
"
    );
    let module =
        naga::front::wgsl::parse_str(&code).unwrap_or_else(|error| {
            panic!("{}", error.emit_to_string(&code))
        });
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .unwrap_or_else(|error| {
        panic!("{}", error.emit_to_string(&code))
    });
}

#[test]
fn a_value_changed_is_a_slot_not_new_code() {
    let mut graph = every_kind();
    let before = compile(&graph).unwrap();
    let colour = graph
        .nodes
        .iter_mut()
        .find(|node| node.does == Does::Kind("colour".to_owned()))
        .unwrap();
    let red = Vec4::new(1.0, 0.0, 0.0, 1.0);
    colour.settings.insert(
        "colour".to_owned(),
        Setting::Fixed(Constant::Color(red)),
    );
    let after = compile(&graph).unwrap();
    assert_eq!(before.code, after.code);
    let values = before.values(&graph, 2.5);
    assert_eq!(values[0].x, 2.5);
    assert!(values.contains(&red));
}
