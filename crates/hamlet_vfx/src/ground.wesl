// The ground: coloured here, point by point, from where it lies, its
// height and its slope, then shaded in brush strokes and lit through soft
// bumps, which show most within the strokes. Its points are laid on the
// patches of `patch.wesl` from the relief pages, in `ground_shape.wesl`,
// which hands on in the vertex colour what the shader cannot know alone:
// the woods in red, the heath in green and in blue how far its patch is
// sunk. Strokes and bumps are one
// texture laid over the world's x and z in a grid of hexagons. Each corner of
// the grid takes the texture moved and turned at random, and every point
// blends the three corners about it, the higher bump winning, so no repeat
// shows. The hexagons follow stochastic hex tiling; the slopes are turned
// with the texture, so the light stays true.
//
// Where the grass's own flowers are too small to show, the ground is
// painted with them instead, a dot to a flower, in the same patches and
// colours, so they are seen to the horizon. Further off the dots grow, so
// none is smaller than a pixel, and fewer, so the ground holds as much of
// their colour.

import bevy_pbr::render::{
    mesh_functions,
    mesh_view_bindings::view,
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{Vertex, VertexOutput, FragmentOutput, decompress_vertex},
    view_transformations::position_world_to_clip,
};
import hamlet_vfx::ground_shape::ground_point;
import hamlet_vfx::noise::{key, scramble, smooth_step, swell};
import hamlet_vfx::soil::{
    BLOOMS, bloom_colour, blooms_at, grass_likely, ground_colour, heads_shown, soil,
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

// `BloomsUniform` in `ground.rs`.
struct Blooms {
    // x: the share of tufts in flower where flowers grow. y: how far from
    // the eye the grass reaches. z: how far apart its tufts are.
    grow: vec4<f32>,
    // x: how brightly they glow, as far into the night as it is. y: the
    // breadth of the patches their glow swells in. z: how fast those drift.
    // w: the time.
    glow: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> ground: Ground;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var bumps: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var bumps_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(106) var<uniform> blooms: Blooms;

// How far a painted flower's head reaches from its middle at its finest,
// against how far apart the tufts are, and the least it reaches, in
// pixels; how many heads a flower has, and how far they spread from its
// middle, against how far apart the tufts are.
const FLOWER: f32 = 0.06;
const FLOWER_PIXELS: f32 = 1.2;
const HEADS: u32 = 3u;
const HEADS_SPREAD: f32 = 0.2;

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

// A point from 0 to 1 each way, from the hash `h`.
fn unit(h: u32) -> vec2<f32> {
    return vec2<f32>(f32(h & 0xffffu), f32(h >> 16u)) / 65535.0;
}

// How much of a painted flower covers `p` on the map, a pixel `pixel`
// metres across, where `share` of the squares the flowers are laid in hold
// one: a few heads about its middle, the squares `2^level` times as wide as
// the tufts lie apart and each head as many times as wide, so every level
// holds as much of their colour.
fn painted_at(p: vec2<f32>, level: u32, share: f32, pixel: f32) -> f32 {
    let side = blooms.grow.z * f32(1u << level);
    let square = floor(p / side);
    let h = key(soil.seed + 93u + level, i32(square.x), i32(square.y));
    if f32(h >> 8u) / 16777216.0 >= share {
        return 0.0;
    }
    var s = scramble(h);
    let middle = (square + 0.3 + 0.4 * unit(s)) * side;
    let reach = side * FLOWER;
    var cover = 0.0;
    for (var k = 0u; k < HEADS; k++) {
        s = scramble(s);
        let head = middle + (unit(s) - 0.5) * 2.0 * HEADS_SPREAD * side;
        let d = distance(p, head);
        cover = max(cover, 1.0 - smoothstep(reach - pixel * 0.5, reach + pixel * 0.5, d));
    }
    return cover;
}

// How much of the flowers painted on the ground covers `p` on the map,
// where `share` of the places a tuft might grow hold one in flower, a pixel
// `pixel` metres across: from the level whose dots are at least
// `FLOWER_PIXELS` across, blended into the next as they near it.
fn painted(p: vec2<f32>, share: f32, pixel: f32) -> f32 {
    let finest = blooms.grow.z * FLOWER;
    let level = clamp(log2(FLOWER_PIXELS * pixel / finest), 0.0, 16.0);
    let low = u32(level);
    return mix(
        painted_at(p, low, share, pixel),
        painted_at(p, low + 1u, share, pixel),
        fract(level),
    );
}

// A point of a patch of the ground, as `ground_shape.wesl` raises it.
@vertex
fn vertex(vertex_in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    // The mesh's attributes may be stored compressed.
    let vertex = decompress_vertex(vertex_in, vertex_in.instance_index);
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    let point = ground_point(vertex.position.xz, world_from_local);
    out.world_position = vec4<f32>(point.position, 1.0);
    out.position = position_world_to_clip(point.position);
    out.world_normal = point.normal;
@if(VERTEX_COLORS)
    out.color = point.colour;
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

    // Under the water the bed is smooth: no strokes, no clods, which the
    // clear water would show as a pattern repeating over the sea floor.
    let dry = smooth_step(soil.water - 0.6, soil.water + 0.1, in.world_position.y + given.z);
    let rough = mix(ground.calm, 1.0, noise(world / ground.patches)) * dry;
    let slope = bump.xy * ground.strength * rough * mix(ground.bare, 1.0, stroke);

    // The bumps' slope over x and z, laid along the surface, tilts the
    // normal away from the rise.
    let n = pbr_input.N;
    let rise = vec3<f32>(slope.x, 0.0, slope.y);
    pbr_input.N = normalize(n - (rise - n * dot(n, rise)));

    let dip = mix(1.0 - ground.cavity * rough, 1.0, bump.z);
    let tone = mix(vec3<f32>(1.0), mix(ground.dark.rgb, ground.light.rgb, stroke), dry);
    var painted_colour = colour * tone * dip;

    // The flowers, where the grass flowers and its own are too small to
    // show.
    let pixel = max(max(length(dpdx(world)), length(dpdy(world))), 1e-4);
    let away = distance(view.world_position, in.world_position.xyz);
    let past = 1.0 - heads_shown(away, blooms.grow.y);
    let likely = grass_likely(in.world_position.y + given.z, normalize(in.world_normal).y, given.x);
    let share = blooms.grow.x * (1.0 - given.x) * likely * past;
    var flower = vec3<f32>(0.0);
    var cover = 0.0;
    if share > 0.0 {
        let field = smooth_step(BLOOMS - 0.02, BLOOMS + 0.02, blooms_at(world));
        if field > 0.0 {
            cover = painted(world, share * field, pixel);
            flower = bloom_colour(world);
            painted_colour = mix(painted_colour, flower, cover);
        }
    }
    pbr_input.material.base_color = vec4<f32>(painted_colour, 1.0);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    // At night they glow as the grass's flowers do.
    if blooms.glow.x > 0.0 && cover > 0.0 {
        let shine = blooms.glow.x * swell(world, blooms.glow.y, blooms.glow.w * blooms.glow.z);
        out.color = vec4<f32>(out.color.rgb + flower * shine * cover, out.color.a);
    }
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
