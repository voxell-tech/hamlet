// Leaves in the prepass and the shadows: moved as `leaves_sway.wesl`
// moves them, and cut where it cuts them, so their depth and shadow have
// the holes the leaves have.

import bevy_pbr::{
    prepass::io::{Vertex, VertexOutput, FragmentOutput, decompress_vertex},
    render::{
        mesh_functions,
        view_transformations::position_world_to_clip,
    },
};
import hamlet_vfx::leaves_sway::{Leaves, swayed, grown, is_cut, stroke_cover};
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
    // A mesh with no UVs has no bark to hold still.
@if(VERTEX_UVS_A)
    let uv = vertex.uv;
@else
    let uv = vec2<f32>(0.0);
@if(VERTEX_UVS_B)
    let at = grown(
        leaves,
        wind,
        (world_from_local * vec4<f32>(vertex.position, 1.0)).xyz,
        vertex.uv_b.x,
    );
@else
    let at = swayed(leaves, wind, vertex.position, uv, world_from_local);
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

// Whether to cut this pixel away, where its stroke does not cover it.
fn cut(in: VertexOutput) -> bool {
@if(VERTEX_UVS_A)
    let cover = stroke_cover(
        textureSample(strokes, strokes_sampler, in.uv).a,
        in.uv,
    );
@else
    let cover = 1.0;
    return is_cut(cover);
}

@if(PREPASS_FRAGMENT)
@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
    if cut(in) {
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
    if cut(in) {
        discard;
    }
}
