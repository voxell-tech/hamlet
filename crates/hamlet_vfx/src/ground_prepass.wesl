// The ground in the prepass: each point of a patch where `ground_shape.wesl`
// raises it, so the depth the water and the light read is the ground's.

import bevy_pbr::{
    prepass::io::{Vertex, VertexOutput, decompress_vertex},
    render::{mesh_functions, view_transformations::position_world_to_clip},
};
import hamlet_vfx::ground_shape::ground_point;

@vertex
fn vertex(vertex_in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let vertex = decompress_vertex(vertex_in, vertex_in.instance_index);
    let world_from_local = mesh_functions::get_world_from_local(vertex_in.instance_index);
    let point = ground_point(vertex.position.xz, world_from_local);
    out.world_position = vec4<f32>(point.position, 1.0);
    out.position = position_world_to_clip(point.position);
@if(UNCLIPPED_DEPTH_ORTHO_EMULATION) {
    out.unclipped_depth = out.position.z;
    out.position.z = min(out.position.z, 1.0);
}
@if(NORMAL_PREPASS_OR_DEFERRED_PREPASS)
    out.world_normal = point.normal;
@if(VERTEX_COLORS)
    out.color = point.colour;
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
