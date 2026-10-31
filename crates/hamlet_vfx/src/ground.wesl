// The ground: coloured here, point by point, from where it lies, its
// height and its slope, then shaded in brush strokes and lit through soft
// bumps, which show most within the strokes. What the shader cannot know
// alone comes in the vertex colour: the woods in red, the heath in green
// and in blue how far the ground stands above where its tile is drawn. Strokes and bumps are one
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
import hamlet_vfx::noise::{fbm, perlin, smooth_step};

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

// The colours the ground is painted with, linear, and what places them.
// The fields are explained on `SoilUniform` in `ground.rs`.
struct Soil {
    grass_shade: vec4<f32>,
    grass: vec4<f32>,
    grass_sun: vec4<f32>,
    grass_high: vec4<f32>,
    rock_dark: vec4<f32>,
    rock: vec4<f32>,
    rock_warm: vec4<f32>,
    snow: vec4<f32>,
    dirt: vec4<f32>,
    dirt_shade: vec4<f32>,
    mud: vec4<f32>,
    water: f32,
    seed: u32,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(103) var<uniform> soil: Soil;

// The colour of the ground at `p` on the map, `y` high, facing `n`, with
// `woods` and `heath` the shares of each there.
fn ground_colour(p: vec2<f32>, y: f32, n: vec3<f32>, woods: f32, heath: f32) -> vec3<f32> {
    // Grass in broad patches of shade and sun, flecked with sun.
    let broad = 0.5 + 0.5 * fbm(p * 0.035, 3u, soil.seed + 80u);
    let fine = 0.5 + 0.5 * perlin(p * 0.33, soil.seed + 81u);
    var c = mix(soil.grass_shade.rgb, soil.grass.rgb, smoothstep(0.3, 0.62, broad));
    c = mix(c, soil.grass_sun.rgb, 0.45 * fine * smoothstep(0.35, 0.7, broad));
    c = mix(c, soil.grass_high.rgb, heath);
    c = mix(c, soil.grass_shade.rgb * 0.9, 0.55 * woods);
    // Thinner, duller grass with height, then rock, then snow.
    c = mix(c, soil.grass_high.rgb, smoothstep(5.0, 18.0, y));
    let grain = perlin(p * 0.21, soil.seed + 82u);
    let steep = 1.0 - n.y;
    let rock = max(
        smoothstep(0.1, 0.24, steep + 0.03 * grain),
        smoothstep(20.0, 34.0, y + 4.0 * grain),
    );
    let stone = mix(
        mix(soil.rock_dark.rgb, soil.rock.rgb, 0.5 + 0.5 * grain),
        soil.rock_warm.rgb,
        smoothstep(-0.3, 0.6, n.x),
    );
    c = mix(c, stone, rock);
    let snow = smoothstep(36.0, 44.0, y + 5.0 * grain) * smooth_step(0.5, 0.25, steep);
    c = mix(c, soil.snow.rgb, snow);
    // A sandy shore, wet at the waterline.
    let shore = smooth_step(soil.water + 0.75, soil.water + 0.2, y);
    c = mix(c, mix(soil.dirt.rgb, soil.dirt_shade.rgb, 0.4), shore * 0.8);
    return mix(c, soil.mud.rgb, smooth_step(soil.water + 0.2, soil.water - 0.3, y));
}

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
@if(VERTEX_COLORS)
    let given = in.color.xyz;
@else
    let given = vec3<f32>(0.0);
    // Faced as the ground itself, before the bumps tilt it.
    let colour = ground_colour(
        world,
        in.world_position.y + given.z,
        normalize(in.world_normal),
        given.x,
        given.y,
    );
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
    pbr_input.material.base_color = vec4<f32>(colour * tone * dip, 1.0);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
