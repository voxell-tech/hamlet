// Leaves: placed as `leaves_sway.wesl` blows them, coloured by their
// vertices and their tree's tint, painted with a stroke and cut to its
// edge. Lit mostly by the sky, they turn toward a cool shade where the
// sun does not reach them, glow at the crown's edge where the sun is
// behind it, as light comes through thin leaves, and keep a rim of the
// night's colour after dark. Far off they fade into the haze.

import bevy_pbr::render::{
    mesh_functions,
    mesh_view_bindings::{fog, lights, view},
    mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT,
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{Vertex, VertexOutput, FragmentOutput, decompress_vertex},
    shadows::fetch_directional_shadow,
    view_transformations::position_world_to_clip,
};
import hamlet_vfx::leaves_sway::{Leaves, swayed, left_open, is_cut};
import hamlet_vfx::wind::Wind;

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> leaves: Leaves;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var<uniform> wind: Wind;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var strokes: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var strokes_sampler: sampler;

// The tint of the tree whose tag is `tag`, as `tint` in `leaves.rs` has
// it. The two must give the same answer.
fn tint(tag: u32) -> vec3<f32> {
    if tag == 0u {
        return vec3<f32>(1.0);
    }
    let a = f32(tag & 0xffu) / 255.0 - 0.5;
    let b = f32((tag >> 8u) & 0xffu) / 255.0 - 0.5;
    let light = 1.0 + 0.22 * a;
    return light * vec3<f32>(1.0 + 0.1 * b, 1.0, 1.0 - 0.12 * b);
}

@vertex
fn vertex(vertex_in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    // The mesh's attributes may be stored compressed.
    let vertex = decompress_vertex(vertex_in, vertex_in.instance_index);
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    let at = swayed(leaves, wind, vertex.position, world_from_local);
    out.world_position = vec4<f32>(at, 1.0);
    out.position = position_world_to_clip(at);
@if(VERTEX_NORMALS)
    out.world_normal = mesh_functions::mesh_normal_local_to_world(
        vertex.normal,
        vertex.instance_index,
    );
@if(VERTEX_UVS_A)
    out.uv = vertex.uv;
@if(VERTEX_COLORS)
    out.color = vec4<f32>(
        vertex.color.rgb * tint(mesh_functions::get_tag(vertex.instance_index)),
        vertex.color.a,
    );
@if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = vertex.instance_index;
@if(VISIBILITY_RANGE_DITHER)
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(
        vertex.instance_index,
        world_from_local[3],
    );
    return out;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
@if(VERTEX_UVS_A)
    let paint = textureSample(strokes, strokes_sampler, in.uv);
@else
    let paint = vec4<f32>(1.0);
    let open = left_open(leaves, in.world_position.xyz, view.world_position);
    if is_cut(paint.a * open, in.position.xy) {
        discard;
    }

    // Both faces of a card face out of the crown, as its normal does,
    // so its back is lit as its front is.
    var pbr_input = pbr_input_from_standard_material(in, true);
    let n = pbr_input.N;
    let v = pbr_input.V;
    let albedo = pbr_input.material.base_color.rgb * paint.r;

    // The sun is the light that casts shadows; the fill casts none.
    var sun = vec3<f32>(0.0);
    var to_sun = vec3<f32>(0.0, 1.0, 0.0);
    var lit = 0.0;
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
        let shadow = fetch_directional_shadow(i, in.world_position, n, view_z, in.position.xy);
        lit = saturate(dot(n, to_sun)) * shadow;
        break;
    }

    // Shaded leaves turn cool, not merely dark, and the sky lights them
    // more than it lights the ground.
    let cool = mix(vec3<f32>(1.0), leaves.shade.rgb, leaves.shade.w);
    pbr_input.material.base_color = vec4<f32>(
        albedo * mix(cool, vec3<f32>(1.0), sqrt(lit)),
        1.0,
    );
    pbr_input.diffuse_occlusion *= leaves.edge.z;

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);

    // With the sun behind the crown, its edge glows in the leaves' own
    // colour, brightest where the eye looks straight at the sun.
    let edge = 1.0 - saturate(dot(n, v));
    let behind = saturate(dot(-v, to_sun));
    let through = leaves.edge.y * pow(behind, 4.0) * (0.35 + 0.65 * edge * edge);
    var colour = out.color.rgb + albedo * sun * view.exposure * through;
    // At night the crown's edge keeps a rim of the night's colour.
    let rim = leaves.rim.rgb * leaves.rim.w * leaves.edge.w * pow(edge, 3.0);
    colour += rim * albedo / max(max(albedo.r, albedo.g), max(albedo.b, 0.05));

    // Far off, the leaves sink into the haze.
    let far = distance(in.world_position.xyz, view.world_position);
    let hazed = smoothstep(leaves.haze.x, leaves.haze.y, far) * leaves.haze.z;
    colour = mix(colour, fog.base_color.rgb, hazed);

    out.color = vec4<f32>(colour, out.color.a);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
