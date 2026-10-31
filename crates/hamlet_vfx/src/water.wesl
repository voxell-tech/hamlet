// Water: the sea, the rivers and every pool, one surface at the water's
// height, moved by the waves of `waves.wesl`, which turn to run in to the
// land near it, with the foam `foam.wesl` keeps on it. Its waves and foam
// are ported from an MIT-licensed water shader; its notice is in
// `water/LICENSE`.
//
// It is painted rather than lit as glass: in clean bands of colour by its
// depth, its crests a step lighter, lit in two tones with the sun caught in
// crisp highlights, and its foam in solid shapes with a darker rim. The
// shallows are clear enough to see the ground through, and at its very edge
// the water fades away; a strip of foam breathes there.

import bevy_pbr::{
    render::{
        mesh_functions,
        pbr_fragment::pbr_input_from_standard_material,
        pbr_functions::main_pass_post_lighting_processing,
        forward_io::{Vertex, VertexOutput, FragmentOutput, decompress_vertex},
        mesh_view_bindings::{lights, view},
        mesh_view_bindings as view_bindings,
        view_transformations::{depth_ndc_to_view_z, position_world_to_clip},
    },
    prepass::utils::prepass_depth,
};
import hamlet_vfx::waves::{FIELD_REACH, varied, wave};

// The fields are explained on `WaterUniform` in `water.rs`.
struct Water {
    shallow: vec4<f32>,
    mid: vec4<f32>,
    deep: vec4<f32>,
    foam_color: vec4<f32>,
    foam: vec4<f32>,
    time: vec4<f32>,
    field: vec4<f32>,
    calm: vec4<f32>,
    wind: vec4<f32>,
    sky: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> water: Water;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var noise: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var noise_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var edge_noise: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var edge_noise_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(105) var sea_foam: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(106) var sea_foam_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(107) var foam_near: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(108) var foam_near_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(109) var foam_far: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(110) var foam_far_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(111) var shore: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(112) var shore_sampler: sampler;

// How far the waves move the point `pos` metres across.
fn moved(pos: vec2<f32>) -> vec3<f32> {
    return wave(
        noise,
        noise_sampler,
        shore,
        shore_sampler,
        water.field,
        pos,
        water.time.x,
        water.time.y,
        water.time.z,
        water.calm.xy,
        water.wind.xy,
        vec2<f32>(0.0),
    );
}

@vertex
fn vertex(vertex_in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    // The mesh's attributes may be stored compressed.
    let vertex = decompress_vertex(vertex_in, vertex_in.instance_index);
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    let flat = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(vertex.position, 1.0)).xyz;

    // Far off, waves smaller than a pixel would only shimmer, and fall
    // between the mesh's points: there the water lies flat.
    let away = distance(flat.xz, view.world_position.xz);
    let scale = water.time.y;
    let kept = 1.0 - smoothstep(25.0 * scale, 55.0 * scale, away);
    let displacement = moved(flat.xz) * kept;
    let at = flat + displacement;
    // The slope is found between this point and two beside it, further
    // apart far off, where finer would only shimmer.
    let apart = mix(water.shallow.w, water.shallow.w * 20.0, smoothstep(5.0, 40.0, away / scale)) * scale;
    let ahead = flat + vec3<f32>(0.0, 0.0, -apart);
    let left = flat + vec3<f32>(-apart, 0.0, 0.0);
    let to_ahead = ahead + moved(ahead.xz) * kept - at;
    let to_left = left + moved(left.xz) * kept - at;

    out.world_position = vec4<f32>(at, 1.0);
    out.position = position_world_to_clip(at);
    out.world_normal = normalize(cross(to_ahead, to_left));
    // Where the point lay before the waves moved it, how far they lifted
    // it, and how far it lies from the eye along the ground.
@if(VERTEX_COLORS)
    out.color = vec4<f32>(flat.xz, displacement.y, away);
@if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = vertex.instance_index;
@if(VISIBILITY_RANGE_DITHER)
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(
        vertex.instance_index,
        world_from_local[3],
    );
    return out;
}

// The foam of a map, `old` and `fresh` the foam's texture finely and
// coarsely laid: fresh foam shows the coarse texture and old the fine.
fn mixed_foam(old: f32, fresh: f32, foam: vec2<f32>) -> vec2<f32> {
    var out = foam;
    let born = smoothstep(0.4, 1.0, mix(old, fresh, foam.x));
    out.x = clamp(foam.x * born, 0.0, 1.0);
    out.y = smoothstep(0.4, 1.0, foam.y * mix(old, fresh, foam.y));
    return out;
}

// How far beyond the surface, along the eye's line, what was drawn at the
// screen's point `uv` lies, where its depth is `depth` and the surface is
// `surface_z` from the eye in view space.
fn through_water(uv: vec2<f32>, depth: f32, surface_z: f32) -> f32 {
    return surface_z - depth_ndc_to_view_z(depth);
}

// Lines of foam at `p` at time `t`, from 0 to 1, riding the crests of the
// waves that run in to the sea's shores, timed as the first of them is in
// `waves.wesl`, so they come in with the waves at any distance, where the
// waves themselves lie flat and the foam's maps do not reach. Each crest
// brings a few thin ripples, the first the brightest and those trailing
// behind it fainter. They grow out of nothing as the waves turn to the land
// and break up along their length.
fn shore_lines(p: vec2<f32>, t: f32, scale: f32) -> f32 {
    let uv = (p - water.field.xy) / water.field.zw;
    if any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) {
        return 0.0;
    }
    let at = textureSampleLevel(shore, shore_sampler, uv, 0.0);
    let d = at.r * FIELD_REACH;
    let bend = water.time.z;
    if at.a <= 0.0 || d <= 0.0 || d >= bend || dot(at.gb, at.gb) < 1e-6 {
        return 0.0;
    }
    let landward = normalize(at.gb);
    let wobble = varied(noise, noise_sampler, p / scale, t, -landward, 0u).y;
    let phase = t * 1.3 + d / scale + wobble;
    var lines = 0.0;
    for (var k = 0u; k < 3u; k++) {
        // Each ripple trails the last by a share of the wave's length,
        // and is fainter. It lies in short pieces with gaps between, each
        // ripple's in other places, slowly shifting.
        let behind = f32(k);
        let crest = sin(phase - behind * 0.75);
        let line = smoothstep(0.93, 0.97, crest);
        let pieces = textureSampleLevel(
            noise,
            noise_sampler,
            p / (3.5 * scale) + vec2<f32>(0.31, 0.67) * behind + t * 0.01,
            0.0,
        ).r;
        let chunk = smoothstep(0.5, 0.58, pieces);
        lines = max(lines, line * chunk * (1.0 - behind * 0.28));
    }
    let near = (1.0 - smoothstep(bend * 0.5, bend, d)) * at.a;
    return lines * near;
}

// The sky seen along `dir`: the haze at the horizon, bluer and deeper
// overhead.
fn sky_seen(dir: vec3<f32>) -> vec3<f32> {
    let up = smoothstep(0.0, 0.6, dir.y);
    return mix(water.sky.rgb, water.sky.rgb * vec3<f32>(0.72, 0.86, 1.05), up);
}

// The farthest a reflected ray is followed, in metres, and the most steps
// taken along it on the screen, where the reflection shows most.
const MIRROR_REACH: f32 = 120.0;
const MIRROR_STEPS: u32 = 48u;
// The fewest pixels one step covers: a reflection on rippled water need not
// be sharp, so it is found at a coarser grain than the screen's.
const MIRROR_GRAIN: f32 = 3.0;

// The point on the screen, in pixels, that the world point seen with clip
// position `clip` falls on.
fn on_screen(clip: vec4<f32>) -> vec2<f32> {
    let uv = clip.xy / clip.w * vec2<f32>(0.5, -0.5) + 0.5;
    return uv * view.viewport.zw + view.viewport.xy;
}

// How far from the eye, in view space, what was drawn at the pixel `coord` is.
@if(DEPTH_PREPASS)
fn scene_z(coord: vec2<f32>) -> f32 {
    return depth_ndc_to_view_z(prepass_depth(vec4<f32>(coord, 0.0, 1.0), 0u));
}

// The colour drawn before the water at `uv` on the screen.
@if(VIEW_TRANSMISSION_TEXTURE)
fn drawn_at(uv: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(
        view_bindings::view_transmission_texture,
        view_bindings::view_transmission_sampler,
        uv,
        0.0,
    ).rgb;
}

// What a ray from `start` along `dir` meets of the solid world drawn before
// the water: its colour, and in alpha how sure the hit is. The ray is
// walked across the screen in `steps` even steps of pixels, its depth
// following it as perspective does, and tested against the depth of what was
// drawn there; a hit is narrowed down between the last two steps. It fades
// at the screen's edges and toward the end of its reach, so where it finds
// nothing the sky shows, with no seam.
@if(DEPTH_PREPASS && VIEW_TRANSMISSION_TEXTURE)
fn mirror_hit(start: vec3<f32>, dir: vec3<f32>, pixel: vec2<f32>, steps: u32) -> vec4<f32> {
    let size = vec2<f32>(textureDimensions(view_bindings::view_transmission_texture));
    let c0 = view.clip_from_world * vec4<f32>(start, 1.0);
    var c1 = view.clip_from_world * vec4<f32>(start + dir * MIRROR_REACH, 1.0);
    // A ray coming back toward the eye is cut short before it passes it.
    let near = 0.2;
    if c1.w < near {
        let cut = (c0.w - near) / max(c0.w - c1.w, 1e-4);
        c1 = mix(c0, c1, cut);
    }
    let s0 = on_screen(c0);
    let s1 = on_screen(c1);
    // One over the clip w, which runs evenly across the screen, so the
    // ray's view depth at any step is `-1 / k`.
    let k0 = 1.0 / c0.w;
    let k1 = 1.0 / c1.w;

    // Where the ray leaves the screen, as a share of its way to `s1`.
    let lo = view.viewport.xy;
    let hi = view.viewport.xy + view.viewport.zw;
    let span = s1 - s0;
    let safe = select(span, vec2<f32>(1e-4), abs(span) < vec2<f32>(1e-4));
    let exits = max((hi - s0) / safe, (lo - s0) / safe);
    let leaves = clamp(min(exits.x, exits.y), 0.0, 1.0);
    let pixels = length(span) * leaves;
    if pixels < 1.0 {
        return vec4<f32>(0.0);
    }
    let stride = max(MIRROR_GRAIN, pixels / f32(steps)) / length(span);
    // Each pixel starts a little further along than its neighbour, so the
    // steps leave no bands; the ripples hide the grain this leaves.
    let jitter = fract(52.982918 * fract(dot(pixel, vec2<f32>(0.06711056, 0.00583715))));

    var last = 0.0;
    var along = stride * jitter;
    for (var i = 0u; i < steps; i++) {
        if along > leaves {
            break;
        }
        let ray = -1.0 / mix(k0, k1, along);
        let scene = scene_z(mix(s0, s1, along));
        // How far the ray's depth moved in this step: anything thinner
        // than that could be stepped over, so a hit may be that deep.
        let moved = abs(ray + 1.0 / mix(k0, k1, last));
        if ray < scene && ray > scene - (moved + 0.3 + 0.01 * -scene) {
            var a = last;
            var b = along;
            for (var n = 0u; n < 5u; n++) {
                let mid = (a + b) * 0.5;
                if -1.0 / mix(k0, k1, mid) < scene_z(mix(s0, s1, mid)) {
                    b = mid;
                } else {
                    a = mid;
                }
            }
            let at = mix(s0, s1, b);
            // A little blurred: the reflection need not be sharper than
            // the grain it was found at.
            let uv = at / size;
            let blur = 1.5 / size;
            let colour = (
                drawn_at(uv + blur)
                + drawn_at(uv - blur)
                + drawn_at(uv + vec2<f32>(blur.x, -blur.y))
                + drawn_at(uv + vec2<f32>(-blur.x, blur.y))
            ) * 0.25;
            let screen = (at - lo) / view.viewport.zw;
            let edge = min(min(screen.x, 1.0 - screen.x), min(screen.y, 1.0 - screen.y));
            let sure = smoothstep(0.0, 0.1, edge) * (1.0 - smoothstep(0.7, 1.0, b));
            return vec4<f32>(colour, sure);
        }
        last = along;
        along += stride;
    }
    return vec4<f32>(0.0);
}

// 1 above `edge` and 0 below, with an edge a pixel wide wherever it is seen,
// so that foam's edges neither alias nor blur.
fn aa_step(edge: f32, v: f32) -> f32 {
    let width = clamp(fwidth(v), 1e-4, 0.25);
    return clamp((v - edge) / width + 0.5, 0.0, 1.0);
}

// The depth drawn at the screen's point `uv`, from 0 far off to 1 near.
fn depth_at(uv: vec2<f32>) -> f32 {
    return prepass_depth(vec4<f32>(uv * view.viewport.zw + view.viewport.xy, 0.0, 1.0), 0u);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
@if(VERTEX_COLORS)
    let carried = in.color;
@else
    let carried = vec4<f32>(in.world_position.xz, 0.0, distance(in.world_position.xz, view.world_position.xz));
    let world_xz = carried.xy;
    let away = carried.w;
    let t = water.time.x;
    let scale = water.time.y;

    // The foam, from the near map close by and the far one beyond it, and
    // none past that.
    var foam = vec2<f32>(0.0);
    if water.foam.z > 0.0 {
        let world_uv = world_xz / 15.0;
        let fresh = textureSample(sea_foam, sea_foam_sampler, world_uv * 2.0).r;
        let old = textureSample(sea_foam, sea_foam_sampler, world_uv * 24.0).r;
        let near = textureSample(foam_near, foam_near_sampler, (world_xz - water.foam.xy) / water.foam.z + 0.5).rg;
        let far = textureSample(foam_far, foam_far_sampler, (world_xz - water.foam.xy) / water.foam.w + 0.5).rg;
        let fade_near = smoothstep(water.foam.z * 0.35, water.foam.z * 0.5, away);
        let fade_far = smoothstep(water.foam.w * 0.45, water.foam.w * 0.5, away);
        foam = mix(mixed_foam(old, fresh, near), mixed_foam(old, fresh, far), fade_near);
        foam = mix(foam, vec2<f32>(0.0), fade_far);
    }

    // The slope is found again for each point drawn, from the waves
    // themselves, so that far off, where the mesh lies flat, the water
    // still shows them. It fades where a pixel covers several waves, and
    // is found between points a pixel apart there, so that it does not
    // shimmer.
    let pixel = length(fwidth(world_xz));
    let apart = max(0.5 * scale, pixel);
    let here = vec3<f32>(world_xz.x, 0.0, world_xz.y) + moved(world_xz);
    let ahead = vec3<f32>(world_xz.x, 0.0, world_xz.y - apart) + moved(world_xz - vec2<f32>(0.0, apart));
    let left = vec3<f32>(world_xz.x - apart, 0.0, world_xz.y) + moved(world_xz - vec2<f32>(apart, 0.0));
    let detail = (1.0 - smoothstep(0.4, 1.8, pixel / scale)) * water.calm.w;
    let N = normalize(mix(vec3<f32>(0.0, 1.0, 0.0), normalize(cross(ahead - here, left - here)), detail));

    // What lies under the surface, bent by its slope as the eye sees it,
    // less so far off and where the water is thin, and how far beyond the
    // surface it lies. Where the bent point falls on nothing, the sky,
    // nothing shows through.
    var under = vec3<f32>(0.0);
    var through = 64.0;
    // How deep the water stands over the ground below, straight down, the
    // same however it is looked at.
    var below = 64.0;
@if(DEPTH_PREPASS && VIEW_TRANSMISSION_TEXTURE) {
    let seen = (view.view_from_world * vec4<f32>(N, 0.0)).xyz;
    let bend = vec2<f32>(seen.x, -seen.y) * vec2<f32>(0.025, 0.01) * 1.25;
    let surface_z = (view.view_from_world * in.world_position).z;
    let less_far = clamp(4.0 / -surface_z, 0.0, 1.0);
    let screen_uv = (in.position.xy - view.viewport.xy) / view.viewport.zw;
    var uv = screen_uv + bend * less_far;
    through = through_water(uv, depth_at(uv), surface_z);
    let thin = smoothstep(0.0, 0.5, through) * 0.8;
    uv = screen_uv + bend * less_far * thin;
    let depth = depth_at(uv);
    through = max(through_water(uv, depth, surface_z), 0.0);
    if depth > 0.0 {
        under = textureSampleLevel(
            view_bindings::view_transmission_texture,
            view_bindings::view_transmission_sampler,
            uv,
            0.0,
        ).rgb;
        let clip = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, depth, 1.0);
        let bed = view.world_from_clip * clip;
        below = max(in.world_position.y - bed.y / bed.w, 0.0);
    }
}

    // A strip of foam where the water lies thinnest over the ground,
    // breathing wider and narrower, ragged along its length.
    let ragged = textureSample(edge_noise, edge_noise_sampler, world_xz * 2.0 / scale + t * 0.03).r;
    let width = (0.05 + sin(t * 0.5) * 0.02 + ragged * 0.04) * scale;
    var strip = 0.0;
    if below > 0.0 && below < width {
        let fade_in = smoothstep(0.0, 0.1 * width, below);
        let fade_out = 1.0 - smoothstep(0.8 * width, width, below);
        strip = fade_in * fade_out;
    }
    let lines = shore_lines(world_xz, t, scale);

    // Painted in bands by depth: pale in the shallows, a middle teal, and
    // deep blue, each a clean step from the last. The shallows are clear
    // enough to see the ground through, and the crests a step lighter.
    let deep = below / scale;
    let first = smoothstep(water.mid.w * 0.85, water.mid.w * 1.15, deep);
    let second = smoothstep(water.deep.w * 0.85, water.deep.w * 1.15, deep);
    var body = mix(water.shallow.rgb, water.mid.rgb, first);
    body = mix(body, water.deep.rgb, second);
    let crest = smoothstep(0.25, 0.32, carried.z / scale);
    body = mix(body, body * 1.18 + 0.02, crest * 0.7);

    // Lit in two tones, the side of a wave turned from the sun a step
    // darker, and the sun caught in crisp highlights.
    let V = normalize(view.world_position - in.world_position.xyz);
    var light = lights.ambient_color.rgb * view.exposure;
    var glint = vec3<f32>(0.0);
    for (var i = 0u; i < lights.n_directional_lights; i++) {
        let sun = lights.directional_lights[i];
        let L = sun.direction_to_light;
        let light_in = sun.color.rgb * view.exposure / 3.14159265;
        let tone = mix(0.72, 1.0, smoothstep(0.38, 0.44, dot(N, L)));
        light += light_in * max(L.y, 0.0) * tone;
        let H = normalize(L + V);
        glint += light_in * smoothstep(0.993, 0.996, dot(N, H)) * 2.5;
    }
    var colour = mix(body * light, under, water.foam_color.w * (1.0 - first));

    // What the water mirrors: the land and all on it where the reflected
    // ray meets them, else the sky. The more the flatter the eye looks, and
    // never more than half, so it can still be seen into.
    let facing = clamp(dot(V, N), 0.0, 1.0);
    let weight = min(pow(1.0 - facing, 5.0) * water.sky.w, 0.5);
    var R = reflect(-V, N);
    R.y = abs(R.y);
    var mirrored = sky_seen(R);
@if(DEPTH_PREPASS && VIEW_TRANSMISSION_TEXTURE) {
    if weight > 0.01 {
        // A faint reflection is found in fewer, coarser steps: what it
        // misses is hidden in how little of it shows.
        let steps = u32(mix(16.0, f32(MIRROR_STEPS), smoothstep(0.02, 0.3, weight)));
        let hit = mirror_hit(in.world_position.xyz, R, in.position.xy, steps);
        mirrored = mix(mirrored, hit.rgb, hit.a);
    }
}
    colour = mix(colour, mirrored, weight);

    // Foam in crisp, solid shapes with a darker rim, drawn from how much of
    // it there is.
    let froth = max(max(max(foam.x, foam.y), strip), lines);
    let core = aa_step(0.45, froth);
    let ring = aa_step(0.25, froth) - core;
    let white = water.foam_color.rgb * light;
    colour = mix(colour, white * 0.78, ring);
    colour = mix(colour, white, core);
    colour += glint * (1.0 - core);

    // Where the water lies thinnest over the ground it fades away, so the
    // ground runs into it with no hard edge; its foam stays.
    let shown = max(smoothstep(0.0, water.calm.z, below), max(core, ring));
    colour = mix(under, colour, shown);

    var out: FragmentOutput;
    out.color = main_pass_post_lighting_processing(pbr_input, vec4<f32>(colour, 1.0));
    return out;
}
