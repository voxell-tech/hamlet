// Grass: each blade grown by `grass_shape.wesl`, coloured by its vertices,
// lit as the ground under it is, darker at its root where the light does
// not reach, and glowing at its tip with the sun behind it, as light comes
// through thin blades.

import bevy_pbr::render::{
    mesh_functions,
    mesh_view_bindings::{lights, view},
    mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT,
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{Vertex, VertexOutput, FragmentOutput, decompress_vertex},
    shadows::fetch_directional_shadow,
    view_transformations::position_world_to_clip,
};
import hamlet_vfx::grass_shape::{grass, blade};

@vertex
fn vertex(vertex_in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let vertex = decompress_vertex(vertex_in, vertex_in.instance_index);
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    let b = blade(vertex.position, vertex.uv.x, vertex.color, world_from_local);
    out.world_position = vec4<f32>(b.position, 1.0);
    out.position = position_world_to_clip(b.position);
    out.world_normal = b.normal;
    // How far up the blade the point is.
    out.uv = vec2<f32>(vertex.position.y, 0.0);
    out.color = vec4<f32>(b.colour, 1.0);
@if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = vertex.instance_index;
    return out;
}

@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
    // Both faces of a blade are lit as the ground is.
    var pbr_input = pbr_input_from_standard_material(in, true);
    pbr_input.N = normalize(in.world_normal);
    let up = in.uv.x;
    let albedo = pbr_input.material.base_color.rgb;
    pbr_input.diffuse_occlusion *= mix(0.45, 1.0, up);

    // The sun is the light that casts shadows; the fill casts none.
    var sun = vec3<f32>(0.0);
    var to_sun = vec3<f32>(0.0, 1.0, 0.0);
    var shadow = 0.0;
    let view_z = dot(vec4<f32>(
        view.view_from_world[0].z,
        view.view_from_world[1].z,
        view.view_from_world[2].z,
        view.view_from_world[3].z,
    ), in.world_position);
    for (var i = 0u; i < lights.n_directional_lights; i += 1u) {
        let light = lights.directional_lights[i];
        if (light.flags & DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) == 0u {
            continue;
        }
        sun = light.color.rgb;
        to_sun = light.direction_to_light;
        shadow = fetch_directional_shadow(i, in.world_position, pbr_input.N, view_z, in.position.xy);
        break;
    }

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    // With the sun behind them, the tips glow in their own colour.
    let behind = saturate(dot(-pbr_input.V, to_sun));
    let through = grass.look.w * pow(behind, 3.0) * up * up * shadow;
    out.color = vec4<f32>(out.color.rgb + albedo * sun * view.exposure * through, out.color.a);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
