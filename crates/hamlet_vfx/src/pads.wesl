// Lily pads: each ridden as `pads_ride.wesl` has it, coloured by its
// vertices, painted with a stroke, cut to its edge and lit.

import bevy_pbr::render::{
    mesh_functions,
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{Vertex, VertexOutput, FragmentOutput, decompress_vertex},
    view_transformations::position_world_to_clip,
};
import hamlet_vfx::leaves_sway::{is_cut, stroke_cover};
import hamlet_vfx::pads_ride::{ridden, strokes, strokes_sampler};

@vertex
fn vertex(vertex_in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let vertex = decompress_vertex(vertex_in, vertex_in.instance_index);
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    let at = ridden(vertex.position, world_from_local);
    out.world_position = vec4<f32>(at, 1.0);
    out.position = position_world_to_clip(at);
    out.world_normal = mesh_functions::mesh_normal_local_to_world(vertex.normal, vertex.instance_index);
    out.uv = vertex.uv;
    out.color = vertex.color;
@if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = vertex.instance_index;
    return out;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    let paint = textureSample(strokes, strokes_sampler, in.uv);
    if is_cut(stroke_cover(paint.a, in.uv)) {
        discard;
    }
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color = vec4<f32>(pbr_input.material.base_color.rgb * paint.r, 1.0);
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
