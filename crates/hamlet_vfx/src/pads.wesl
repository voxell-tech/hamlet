// Lily pads: each ridden as `pads_ride.wesl` has it, coloured by its
// vertices, painted with a stroke, cut to its edge and shaded. At night the
// flower glows in its colour, swelling and fading as the grass's flowers
// do.

import bevy_pbr::render::{
    mesh_functions,
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{Vertex, VertexOutput, FragmentOutput, decompress_vertex},
    view_transformations::position_world_to_clip,
};
import hamlet_vfx::leaves_sway::{is_cut, stroke_cover};
import hamlet_vfx::noise::swell;
import hamlet_vfx::pads_ride::{ride, ridden, turned, strokes, strokes_sampler, water};

// `PadExt::glow` in `pads.rs`. x: how brightly the flowers glow, as far
// into the night as it is. y: the breadth of the patches their glow swells
// in. z: how fast they drift.
@group(constants::MATERIAL_BIND_GROUP) @binding(117) var<uniform> glow: vec4<f32>;

@vertex
fn vertex(vertex_in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let vertex = decompress_vertex(vertex_in, vertex_in.instance_index);
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    let riding = ride(world_from_local);
    let at = ridden(vertex.position, world_from_local, riding);
    out.world_position = vec4<f32>(at, 1.0);
    out.position = position_world_to_clip(at);
    let normal = mesh_functions::mesh_normal_local_to_world(vertex.normal, vertex.instance_index);
    out.world_normal = turned(normal, riding.up);
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
    // The flower is marked by its colour's alpha being 0.
    let flower = 1.0 - in.color.a;
    if glow.x > 0.0 && flower > 0.0 {
        let shine = glow.x * swell(in.world_position.xz, glow.y, water.time.x * glow.z);
        out.color = vec4<f32>(out.color.rgb + in.color.rgb * shine * flower, out.color.a);
    }
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
