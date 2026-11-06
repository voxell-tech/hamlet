// Grass: each tuft a list holds, placed by `grass_shape.wesl`, coloured by
// its vertices, shaded by the ground's normal under it, darker at its roots
// where the light does not reach, and with a sheen seen low against the sun.
// Its flowers, and faintly the grass about them, glow at night.

import bevy_pbr::render::{
    mesh_types::MESH_FLAGS_SHADOW_RECEIVER_BIT,
    mesh_view_bindings::{lights, view},
    pbr_functions::{apply_pbr_lighting, calculate_view, main_pass_post_lighting_processing},
    pbr_types::{
        STANDARD_MATERIAL_FLAGS_ALPHA_MODE_OPAQUE, STANDARD_MATERIAL_FLAGS_FOG_ENABLED_BIT,
        pbr_input_new,
    },
    view_transformations::position_world_to_clip,
};
import hamlet_vfx::grass_shape::{Tuft, grass, blade};

// Where a list's tufts begin in the lists. `First` in `grass/draw.rs`.
struct First {
    tuft: u32,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(110) var<storage, read> tufts: array<Tuft>;
@group(constants::MATERIAL_BIND_GROUP) @binding(111) var<uniform> first: First;

struct Vertex {
    @builtin(instance_index) tuft: u32,
    @location(0) position: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(5) colour: vec4<f32>,
}

struct Drawn {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec4<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) colour: vec3<f32>,
    // The light it gives off, and how far up its blade it is.
    @location(3) glow: vec4<f32>,
}

@vertex
fn vertex(in: Vertex) -> Drawn {
    let tuft = tufts[first.tuft + in.tuft];
    let b = blade(in.position, in.uv.x, in.uv.y, in.colour.rgb, in.colour.a < 0.5, tuft);
    var out: Drawn;
    out.world_position = vec4<f32>(b.position, 1.0);
    out.position = position_world_to_clip(b.position);
    out.normal = b.normal;
    out.colour = b.colour;
    out.glow = vec4<f32>(b.glow, in.uv.x);
    return out;
}

@fragment
fn fragment(in: Drawn) -> @location(0) vec4<f32> {
    var pbr = pbr_input_new();
    pbr.material.base_color = vec4<f32>(in.colour, 1.0);
    pbr.material.perceptual_roughness = 0.9;
    pbr.material.reflectance = vec3<f32>(0.15);
    pbr.material.flags = STANDARD_MATERIAL_FLAGS_ALPHA_MODE_OPAQUE | STANDARD_MATERIAL_FLAGS_FOG_ENABLED_BIT;
    pbr.flags = MESH_FLAGS_SHADOW_RECEIVER_BIT;
    pbr.is_orthographic = view.clip_from_view[3].w == 1.0;
    pbr.frag_coord = in.position;
    pbr.world_position = in.world_position;
    let n = normalize(in.normal);
    pbr.world_normal = n;
    pbr.N = n;
    pbr.V = calculate_view(in.world_position, pbr.is_orthographic);
    let up = in.glow.w;
    pbr.diffuse_occlusion *= mix(0.55, 1.0, up);

    var colour = apply_pbr_lighting(pbr);

    // A sheen where the eye is low over the ground, toward the sun.
    if lights.n_directional_lights > 0u {
        let light = lights.directional_lights[0];
        let l = light.direction_to_light;
        let v = pbr.V;
        let low = pow(1.0 - saturate(dot(n, v)), 5.0);
        let against = pow(1.0 - saturate(dot(l, v)), 3.0);
        let sheen = grass.shade.w * low * against * saturate(dot(n, l) + 0.3);
        colour = vec4<f32>(colour.rgb + in.colour * light.color.rgb * view.exposure * sheen, colour.a);
    }
    // Flowers, and the grass about them, glow in the flowers' colour at
    // night.
    colour = vec4<f32>(colour.rgb + in.glow.rgb, colour.a);
    return main_pass_post_lighting_processing(pbr, colour);
}
