// A flag in the wind: cloth that hangs from its hoist at x = 0 and is
// blown along x. UV.x is how far a point is along the cloth, 0 at the hoist
// and 1 at the fly. The same shader places it for the depth and shadow
// passes, so its shadow waves with it.

import bevy_pbr::render::{
    mesh_functions,
    view_transformations::position_world_to_clip,
};
import hamlet_vfx::wind::{Wind, strength};

@if(PREPASS_PIPELINE)
import bevy_pbr::prepass::io::{Vertex, VertexOutput, decompress_vertex};
@else
import bevy_pbr::render::forward_io::{Vertex, VertexOutput, decompress_vertex};

struct Flag {
    // x: time in seconds. y: how fast the waves run. z: how far the fly
    // end swings, as a share of the cloth's length. w: waves along it.
    wave: vec4<f32>,
    // x: the cloth's length. The rest is unused.
    shape: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> flag: Flag;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var<uniform> wind: Wind;

// Where a point of the cloth is blown to, and the slope of the cloth
// there across the wind. It swings wider the harder the wind blows.
fn blown(at: vec3<f32>, u: f32, own: f32, strong: f32) -> vec4<f32> {
    let long = max(flag.shape.x, 0.001);
    let swing = flag.wave.z * long * strong;
    let turn = 6.2832 * flag.wave.w;
    let a = turn * u - flag.wave.y * flag.wave.x + own;
    let b = a * 1.7 + 1.3;
    // The hoist is held; the cloth is freer the further along it is.
    let z = swing * u * (sin(a) + 0.35 * sin(b));
    let slope = swing / long
        * (sin(a) + 0.35 * sin(b)
            + u * turn * (cos(a) + 0.35 * 1.7 * cos(b)));
    var p = at;
    p.z += z;
    // It lifts and shortens a little as it swings out.
    p.y += swing * 0.22 * u * cos(a * 0.5);
    p.x -= swing * 0.35 * u * u * (1.0 - cos(a)) * 0.5;
    return vec4<f32>(p, slope);
}

@vertex
fn vertex(vertex_in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    // The mesh's attributes may be stored compressed.
    let vertex = decompress_vertex(vertex_in, vertex_in.instance_index);
    let world_from_local =
        mesh_functions::get_world_from_local(vertex.instance_index);
    // Flags in different places are at different moments of their wave.
    let own = world_from_local[3].x * 0.37 + world_from_local[3].z * 0.23;
@if(VERTEX_UVS_A)
    let u = vertex.uv.x;
@else
    let u = 0.0;
    let moved = blown(
        vertex.position,
        u,
        own,
        strength(wind, world_from_local[3].xz),
    );

    out.world_position = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4<f32>(moved.xyz, 1.0),
    );
    out.position = position_world_to_clip(out.world_position.xyz);

@if(VERTEX_UVS_A)
    out.uv = vertex.uv;
@if(VERTEX_UVS_B)
    out.uv_b = vertex.uv_b;
@if(VERTEX_COLORS)
    out.color = vertex.color;

@if(PREPASS_PIPELINE) {
@if(UNCLIPPED_DEPTH_ORTHO_EMULATION) {
    out.unclipped_depth = out.position.z;
    out.position.z = min(out.position.z, 1.0);
}
@if(NORMAL_PREPASS_OR_DEFERRED_PREPASS && VERTEX_NORMALS)
    out.world_normal = mesh_functions::mesh_normal_local_to_world(
        normalize(vec3<f32>(-moved.w, 0.0, 1.0)) * sign(vertex.normal.z),
        vertex.instance_index,
    );
@if(MOTION_VECTOR_PREPASS)
    out.previous_world_position =
        mesh_functions::mesh_position_local_to_world(
            mesh_functions::get_previous_world_from_local(
                vertex.instance_index,
            ),
            vec4<f32>(moved.xyz, 1.0),
        );
} @else {
@if(VERTEX_NORMALS)
    out.world_normal = mesh_functions::mesh_normal_local_to_world(
        normalize(vec3<f32>(-moved.w, 0.0, 1.0)) * sign(vertex.normal.z),
        vertex.instance_index,
    );
}

@if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = vertex.instance_index;
@if(VISIBILITY_RANGE_DITHER)
    out.visibility_range_dither =
        mesh_functions::get_visibility_range_dither_level(
            vertex.instance_index,
            world_from_local[3],
        );
    return out;
}
