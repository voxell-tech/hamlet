// Grass in the prepass: each tuft where `grass_shape.wesl` places it, so
// the depth the water, the shade and the main pass read is the grass's.

import bevy_pbr::{
    prepass::io::{Vertex, VertexOutput, decompress_vertex},
    render::{mesh_functions, view_transformations::position_world_to_clip},
};
import hamlet_vfx::grass_shape::blade;

@vertex
fn vertex(vertex_in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let vertex = decompress_vertex(vertex_in, vertex_in.instance_index);
    let world_from_local = mesh_functions::get_world_from_local(vertex_in.instance_index);
    let b = blade(
        vertex.position,
        vertex.uv.x,
        vertex.uv.y,
        vertex.color.rgb,
        vertex.color.a < 0.5,
        vertex.uv_b,
        world_from_local,
    );
    out.world_position = vec4<f32>(b.position, 1.0);
    out.position = position_world_to_clip(b.position);
@if(UNCLIPPED_DEPTH_ORTHO_EMULATION) {
    out.unclipped_depth = out.position.z;
    out.position.z = min(out.position.z, 1.0);
}
@if(NORMAL_PREPASS_OR_DEFERRED_PREPASS)
    out.world_normal = b.normal;
@if(VERTEX_UVS_A)
    out.uv = vertex.uv;
@if(VERTEX_COLORS)
    out.color = vec4<f32>(b.colour, 1.0);
@if(MOTION_VECTOR_PREPASS)
    out.previous_world_position = out.world_position;
@if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = vertex_in.instance_index;
    return out;
}
