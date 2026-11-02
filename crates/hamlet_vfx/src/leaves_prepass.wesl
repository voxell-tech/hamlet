// Leaves in the prepass and the shadows: moved as `leaves_sway.wesl`
// moves them, and cut where it cuts them, so their depth and shadow have
// the holes the leaves have. The opening about what the eye watches is
// cut only from the eye's depth, not from the sun's shadow.

import bevy_pbr::{
    prepass::io::{Vertex, VertexOutput, FragmentOutput, decompress_vertex},
    render::{
        mesh_functions,
        mesh_view_bindings::view,
        view_transformations::position_world_to_clip,
    },
};
import hamlet_vfx::leaves_sway::{Leaves, swayed, left_open, is_cut};
import hamlet_vfx::wind::Wind;

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> leaves: Leaves;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var<uniform> wind: Wind;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var strokes: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var strokes_sampler: sampler;

@vertex
fn vertex(vertex_in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let vertex = decompress_vertex(vertex_in, vertex_in.instance_index);
    let world_from_local = mesh_functions::get_world_from_local(vertex_in.instance_index);
    let at = swayed(leaves, wind, vertex.position, world_from_local);
    out.world_position = vec4<f32>(at, 1.0);
    out.position = position_world_to_clip(at);
@if(UNCLIPPED_DEPTH_ORTHO_EMULATION) {
    out.unclipped_depth = out.position.z;
    out.position.z = min(out.position.z, 1.0);
}
@if(VERTEX_UVS_A)
    out.uv = vertex.uv;
@if(NORMAL_PREPASS_OR_DEFERRED_PREPASS && VERTEX_NORMALS)
    out.world_normal = mesh_functions::mesh_normal_local_to_world(
        vertex.normal,
        vertex_in.instance_index,
    );
@if(VERTEX_COLORS)
    out.color = vertex.color;
@if(MOTION_VECTOR_PREPASS)
    out.previous_world_position = out.world_position;
@if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = vertex_in.instance_index;
@if(VISIBILITY_RANGE_DITHER)
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(
        vertex_in.instance_index,
        world_from_local[3],
    );
    return out;
}

// How much of the leaf this pixel shows: its stroke's cover, less the
// opening when the view is the eye's. The sun's views are orthographic,
// the eye's is not.
fn shown(in: VertexOutput) -> f32 {
@if(VERTEX_UVS_A)
    var cover = textureSample(strokes, strokes_sampler, in.uv).a;
@else
    var cover = 1.0;
    if view.clip_from_view[3][3] != 1.0 {
        cover *= left_open(leaves, in.world_position.xyz, view.world_position);
    }
    return cover;
}

@if(PREPASS_FRAGMENT)
@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
    if is_cut(shown(in), in.position.xy) {
        discard;
    }
    var out: FragmentOutput;
@if(NORMAL_PREPASS)
    out.normal = vec4<f32>(normalize(in.world_normal) * 0.5 + vec3<f32>(0.5), 1.0);
@if(MOTION_VECTOR_PREPASS)
    out.motion_vector = vec2<f32>(0.0);
@if(UNCLIPPED_DEPTH_ORTHO_EMULATION)
    out.frag_depth = in.unclipped_depth;
    return out;
}
@else
@fragment
fn fragment(in: VertexOutput) {
    if is_cut(shown(in), in.position.xy) {
        discard;
    }
}
