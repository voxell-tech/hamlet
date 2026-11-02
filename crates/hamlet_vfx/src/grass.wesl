// Grass: each tuft placed by `grass_shape.wesl`, coloured by its vertices,
// lit as the ground under it is, darker at its roots where the light does
// not reach, and with a sheen seen low against the sun. Its flowers glow
// at night.

import bevy_pbr::render::{
    mesh_functions,
    mesh_view_bindings::{lights, view},
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{Vertex, VertexOutput, FragmentOutput, decompress_vertex},
    view_transformations::position_world_to_clip,
};
import hamlet_vfx::grass_shape::{grass, blade};

@vertex
fn vertex(vertex_in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let vertex = decompress_vertex(vertex_in, vertex_in.instance_index);
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
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
    // A hair nearer than the prepass put it, so that its depth always
    // passes the prepass's however the two shaders round.
    out.position.z *= 1.00002;
    out.world_normal = b.normal;
    // How far up its blade, and how brightly it glows.
    out.uv = vec2<f32>(vertex.uv.x, b.glow);
    out.color = vec4<f32>(b.colour, 1.0);
@if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = vertex.instance_index;
    return out;
}

@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, true);
    let n = normalize(in.world_normal);
    pbr_input.N = n;
    let up = in.uv.x;
    let albedo = pbr_input.material.base_color.rgb;
    pbr_input.diffuse_occlusion *= mix(0.55, 1.0, up);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);

    // A sheen where the eye looks low over the ground toward the sun.
    if lights.n_directional_lights > 0u {
        let light = lights.directional_lights[0];
        let l = light.direction_to_light;
        let v = pbr_input.V;
        let low = pow(1.0 - saturate(dot(n, v)), 5.0);
        let against = pow(1.0 - saturate(dot(l, v)), 3.0);
        let sheen = grass.look.w * low * against * saturate(dot(n, l) + 0.3);
        out.color = vec4<f32>(out.color.rgb + albedo * light.color.rgb * view.exposure * sheen, out.color.a);
    }
    // Flowers glow in their own colour at night.
    out.color = vec4<f32>(out.color.rgb + albedo * in.uv.y, out.color.a);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
