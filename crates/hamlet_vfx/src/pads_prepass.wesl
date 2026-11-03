// Lily pads in the prepass: ridden as `pads_ride.wesl` has it, and cut
// where the main pass cuts them.

import bevy_pbr::{
    prepass::io::{Vertex, VertexOutput, FragmentOutput, decompress_vertex},
    render::{mesh_functions, view_transformations::position_world_to_clip},
};
import hamlet_vfx::leaves_sway::{is_cut, stroke_cover};
import hamlet_vfx::pads_ride::{ride, ridden, turned, strokes, strokes_sampler};

@vertex
fn vertex(vertex_in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let vertex = decompress_vertex(vertex_in, vertex_in.instance_index);
    let world_from_local = mesh_functions::get_world_from_local(vertex_in.instance_index);
    let riding = ride(world_from_local);
    let at = ridden(vertex.position, world_from_local, riding);
    out.world_position = vec4<f32>(at, 1.0);
    out.position = position_world_to_clip(at);
@if(UNCLIPPED_DEPTH_ORTHO_EMULATION) {
    out.unclipped_depth = out.position.z;
    out.position.z = min(out.position.z, 1.0);
}
@if(VERTEX_UVS_A)
    out.uv = vertex.uv;
@if(NORMAL_PREPASS_OR_DEFERRED_PREPASS && VERTEX_NORMALS)
    out.world_normal = turned(mesh_functions::mesh_normal_local_to_world(vertex.normal, vertex_in.instance_index), riding.up);
@if(VERTEX_COLORS)
    out.color = vertex.color;
@if(MOTION_VECTOR_PREPASS)
    out.previous_world_position = out.world_position;
@if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = vertex_in.instance_index;
    return out;
}

// Whether to cut this pixel away, where its stroke does not cover it.
fn cut(in: VertexOutput) -> bool {
@if(VERTEX_UVS_A)
    let cover = stroke_cover(textureSample(strokes, strokes_sampler, in.uv).a, in.uv);
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
