// Water: the sea, the rivers and every pool, one surface at the water's
// height. How deep it is under each point comes from the depth of what was
// drawn before it, so it is clear and pale over the shallows, where light
// ripples on the bed, and dark where it is deep. Foam rolls in toward every
// shore in bands and caps the waves out at sea. Ripples run downwind with
// the world's one wind, catching the sun.

import bevy_pbr::{
    render::{
        pbr_fragment::pbr_input_from_standard_material,
        pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
        forward_io::{VertexOutput, FragmentOutput},
        mesh_view_bindings::view,
        view_transformations::depth_ndc_to_view_z,
    },
    prepass::utils::prepass_depth,
};
import hamlet_vfx::noise::smooth_step;
import hamlet_vfx::wind::{Wind, strength};

// The fields are explained on `WaterUniform` in `water.rs`.
struct Water {
    shallow: vec4<f32>,
    deep: vec4<f32>,
    foam: vec4<f32>,
    depth: vec4<f32>,
    waves: vec4<f32>,
    shore: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> water: Water;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var<uniform> wind: Wind;

fn hash(p: vec2<f32>) -> f32 {
    var h = (bitcast<u32>(i32(p.x)) * 0x8da6b343u) ^ (bitcast<u32>(i32(p.y)) * 0xd8163841u);
    h ^= h >> 13u;
    h *= 0x5bd1e995u;
    h ^= h >> 15u;
    return f32(h & 0xffffffu) / 16777216.0;
}

// Smooth noise from 0 to 1.
fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = p - i;
    let u = f * f * (3.0 - 2.0 * f);
    return mix(
        mix(hash(i), hash(i + vec2<f32>(1.0, 0.0)), u.x),
        mix(hash(i + vec2<f32>(0.0, 1.0)), hash(i + vec2<f32>(1.0, 1.0)), u.x),
        u.y,
    );
}

// The height of the ripples at `p`: three layers of noise, each larger and
// slower than the next, carried downwind and a little across it, so that
// they cross and never line up.
fn ripples(p: vec2<f32>, t: f32) -> f32 {
    let down = wind.flow.xy;
    let across = vec2<f32>(-down.y, down.x);
    let scale = water.waves.x;
    let speed = water.waves.z;
    var h = noise((p - (down * 1.0 + across * 0.3) * t * speed) * scale) * 0.55;
    h += noise((p - (down * 1.4 - across * 0.4) * t * speed) * scale * 2.3 + 7.1) * 0.3;
    h += noise((p - (down * 2.1 + across * 0.2) * t * speed) * scale * 5.1 + 13.7) * 0.15;
    return h;
}

// Light the waves bring together on a shallow bed: the lines where a net
// of cells meet, drifting and turning.
fn caustics(p: vec2<f32>, t: f32) -> f32 {
    var best = 8.0;
    var next = 8.0;
    let cell = floor(p);
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let c = cell + vec2<f32>(f32(x), f32(y));
            let r = vec2<f32>(hash(c), hash(c + 31.7));
            let at = c + 0.5 + 0.4 * sin(t * 0.9 + 6.2832 * r);
            let d = distance(p, at);
            if d < best {
                next = best;
                best = d;
            } else if d < next {
                next = d;
            }
        }
    }
    return 1.0 - smooth_step(0.0, 0.18, next - best);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let p = in.world_position.xz;
    let t = wind.flow.z;

    // How deep the water is here: the bed is where the ray from the eye
    // through this point met what was drawn before.
    var depth = 1000.0;
    var bed = p;
@if(DEPTH_PREPASS) {
    let surface_z = depth_ndc_to_view_z(in.position.z);
    let bed_z = depth_ndc_to_view_z(prepass_depth(in.position, 0u));
    let along = bed_z / min(surface_z, -1e-4);
    let at = view.world_position + (in.world_position.xyz - view.world_position) * along;
    depth = max(in.world_position.y - at.y, 0.0);
    bed = at.xz;
}

    let deep = 1.0 - exp(-depth / water.depth.x);
    var colour = mix(water.shallow.rgb, water.deep.rgb, deep);

    // Light netted on the bed, where it can be seen.
    // Bent by the ripples above it, so that it never shows as cells.
    let bent = bed * water.shore.w + vec2<f32>(noise(bed * 0.6 + t * 0.2), noise(bed * 0.6 - t * 0.2 + 5.3)) * 1.4;
    let shallows = 1.0 - smooth_step(0.0, water.depth.x * 0.5, depth);
    let net = caustics(bent, t) * shallows * shallows;
    colour += water.foam.rgb * net * water.waves.w;

    // Foam: bands that roll in toward the shore and break up as they
    // come, and caps where the wind is strong out at sea.
    let wobble = noise(p * 0.35 + t * 0.15);
    let bands = 0.5 + 0.5 * sin(depth * water.shore.x * 6.2832 + t * water.depth.w + wobble * 3.0);
    let near = 1.0 - smooth_step(0.0, water.depth.z, depth);
    let lace = smooth_step(0.45, 0.75, noise(p * 1.7 - wind.flow.xy * t * 0.6));
    var foam = near * smooth_step(1.0 - near * 0.8, 1.0, bands) * (0.5 + 0.5 * lace);
    // A thin bright edge where water meets land.
    foam = max(foam, smooth_step(water.shore.y, 0.0, depth) * (0.7 + 0.3 * lace));
    let gust = strength(wind, p);
    let crest = ripples(p * 0.25, t * 0.6);
    let caps = smooth_step(water.shore.z, 1.0, crest * gust) * deep;
    foam = clamp(foam + caps * lace, 0.0, 1.0);
    colour = mix(colour, water.foam.rgb, foam);

    // The ripples tilt the surface, the harder the wind the more.
    let e = 0.15;
    let h0 = ripples(p, t);
    let hx = ripples(p + vec2<f32>(e, 0.0), t);
    let hz = ripples(p + vec2<f32>(0.0, e), t);
    let tilt = water.waves.y * (0.6 + 0.4 * gust) * (1.0 - foam);
    pbr_input.N = normalize(vec3<f32>((h0 - hx) / e * tilt, 1.0, (h0 - hz) / e * tilt));

    // Clear over the shallows, so the bed shows through; whole where it
    // is deep or white with foam.
    let alpha = mix(water.depth.y, 1.0, max(deep, foam));
    pbr_input.material.base_color = vec4<f32>(colour, alpha);
    pbr_input.material.perceptual_roughness = mix(pbr_input.material.perceptual_roughness, 0.9, foam);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
