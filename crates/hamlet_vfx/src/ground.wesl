// The ground: vertex colours shaded in brush strokes and lit through soft
// bumps, which show most within the strokes. Strokes and bumps are one
// texture laid over the world's x and z in a grid of hexagons. Each corner of
// the grid takes the texture moved and turned at random, and every point
// blends the three corners about it, the higher bump winning, so no repeat
// shows. The hexagons follow stochastic hex tiling; the slopes are turned
// with the texture, so the light stays true.

import bevy_pbr::render::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{VertexOutput, FragmentOutput},
};

// The fields are explained on `GroundSettings` in `ground.rs`.
struct Ground {
    dark: vec4<f32>,
    light: vec4<f32>,
    repeat: f32,
    strength: f32,
    cavity: f32,
    patches: f32,
    calm: f32,
    turn: f32,
    bare: f32,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> ground: Ground;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var bumps: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var bumps_sampler: sampler;

fn hash2(p: vec2<f32>) -> vec2<f32> {
    let q = vec2<f32>(dot(p, vec2<f32>(127.1, 311.7)), dot(p, vec2<f32>(269.5, 183.3)));
    return fract(sin(q) * 43758.5453);
}

// Smooth noise from 0 to 1, for the patches.
fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash2(i).x;
    let b = hash2(i + vec2<f32>(1.0, 0.0)).x;
    let c = hash2(i + vec2<f32>(0.0, 1.0)).x;
    let d = hash2(i + vec2<f32>(1.0, 1.0)).x;
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// The texture as the grid's corner `corner` lays it: the bumps' slope along
// x and z in xy, in texture units, their height in z and the strokes in w.
fn copy_at(uv: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>, corner: vec2<f32>) -> vec4<f32> {
    let r = hash2(corner);
    let a = (r.x - 0.5) * ground.turn * 6.2831853;
    let c = cos(a);
    let s = sin(a);
    // Turns a vector by a, and back.
    let turn = mat2x2<f32>(c, s, -s, c);
    let at = turn * uv + r * 17.0;
    let t = textureSampleGrad(bumps, bumps_sampler, at, turn * dx, turn * dy);
    return vec4<f32>(transpose(turn) * (t.xy * 2.0 - 1.0), t.z, t.w);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    let world = in.world_position.xz;
    let uv = world / ground.repeat;
    let dx = dpdx(uv);
    let dy = dpdy(uv);

    // The triangle of the grid the point lies in, its three corners and
    // the weight of each.
    let grid = uv * 3.4641016;
    let skewed = vec2<f32>(grid.x, grid.y * 1.1547005 - grid.x * 0.5773503);
    let base = floor(skewed);
    let f = fract(skewed);
    let z = 1.0 - f.x - f.y;
    let s = step(0.0, -z);
    let s2 = 2.0 * s - 1.0;
    let w = vec3<f32>(-z * s2, s - f.y * s2, s - f.x * s2);
    let a = copy_at(uv, dx, dy, base + vec2<f32>(s, s));
    let b = copy_at(uv, dx, dy, base + vec2<f32>(s, 1.0 - s));
    let c = copy_at(uv, dx, dy, base + vec2<f32>(1.0 - s, s));

    // The higher bump takes over near the middle of its corner, so the
    // blend shows as one ground, not as a smear of three.
    var k = w * (vec3<f32>(a.z, b.z, c.z) + 0.35);
    k = k * k * k * k;
    k = k / (k.x + k.y + k.z);
    let bump = a * k.x + b * k.y + c * k.z;
    let stroke = bump.w;

    let rough = mix(ground.calm, 1.0, noise(world / ground.patches));
    let slope = bump.xy * ground.strength * rough * mix(ground.bare, 1.0, stroke);

    // The bumps' slope over x and z, laid along the surface, tilts the
    // normal away from the rise.
    let n = pbr_input.N;
    let rise = vec3<f32>(slope.x, 0.0, slope.y);
    pbr_input.N = normalize(n - (rise - n * dot(n, rise)));

    let dip = mix(1.0 - ground.cavity * rough, 1.0, bump.z);
    let tone = mix(ground.dark.rgb, ground.light.rgb, stroke);
    pbr_input.material.base_color = vec4<f32>(pbr_input.material.base_color.rgb * tone * dip, pbr_input.material.base_color.a);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
