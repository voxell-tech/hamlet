// Water: the sea, the rivers and every pool, one surface at the water's
// height, moved by the waves of `waves.wesl`, which turn to run in to the
// land near it, with the foam they leave on it, worked out from their past
// where it is drawn. Its waves and foam are ported from an MIT-licensed
// water shader; its notice is in `water/LICENSE`.
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
import hamlet_vfx::ground_shape::Pages;
import hamlet_vfx::patch::slid;
import hamlet_vfx::waves::{
    SHORE_REACH, SHAPES, SLOPE_MOST, TAU, Bend, Coast, bends, coast, crests, groups, noise_rise, wave,
};

// The fields are explained on `WaterUniform` in `water.rs`.
struct Water {
    shallow: vec4<f32>,
    mid: vec4<f32>,
    deep: vec4<f32>,
    foam_color: vec4<f32>,
    foam: vec4<f32>,
    time: vec4<f32>,
    calm: vec4<f32>,
    wind: vec4<f32>,
    sky: vec4<f32>,
    fading: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> water: Water;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var noise: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var noise_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var edge_noise: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var edge_noise_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(105) var sea_foam: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(106) var sea_foam_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(113) var noise_slope: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(114) var noise_slope_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(115) var pages: texture_2d_array<u32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(116) var<uniform> pages_at: Pages;

// How far from the eye, in units of the waves, the water lies flat.
// `FLAT` in `water.rs`.
const FLAT: f32 = 55.0;
// How far from the eye, in units of the waves, each point's own slope
// begins to fade, and where it is gone.
const SLOPE_FADE: vec2<f32> = vec2<f32>(200.0, 450.0);
// How far from the eye, in metres, the depth of the water begins to be
// found from the land's pages rather than from what was drawn under it, and
// where it is found from them alone: past where the ground's finest patches
// end.
const DEPTH_FROM_PAGES: vec2<f32> = vec2<f32>(60.0, 180.0);
// Far off, where the waves have faded, the width in metres of the noise's
// picture the sea's slow swells are laid from, and the most they tilt it.
const FAR_SWELL: f32 = 600.0;
const FAR_TILT: f32 = 0.05;

// How far the waves move the point `pos` metres across.
fn moved(pos: vec2<f32>) -> vec3<f32> {
    return wave(noise, noise_sampler, shore_at(pos), pos, water.time.x, water.time.y, water.wind.xy);
}

@vertex
fn vertex(vertex_in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    // The mesh's attributes may be stored compressed.
    let vertex = decompress_vertex(vertex_in, vertex_in.instance_index);
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    // A patch of the sea, as `patch.rs` lays them: its corner and side
    // are where its square is moved to and how large it is drawn.
    let corner = world_from_local[3].xz;
    let side = world_from_local[0].x;
    let y = world_from_local[3].y;
    let on_map = slid(vertex.position.xz, corner, side, y, view.world_position);
    let flat = vec3<f32>(on_map.x, y, on_map.y);

    // Far off, waves smaller than a pixel would only shimmer, and fall
    // between the mesh's points: there the water lies flat. Measured
    // from the eye itself, so that seen from high over it the whole sea
    // lies flat.
    let scale = water.time.y;
    let kept = 1.0 - smoothstep(25.0 * scale, FLAT * scale, distance(flat, view.world_position));
    // Where it lies flat the waves are not worked out at all. Each
    // fragment finds its own slope, so none is found here.
    var displacement = vec3<f32>(0.0);
    if kept > 0.0 {
        displacement = moved(flat.xz) * kept;
    }
    let at = flat + displacement;

    out.world_position = vec4<f32>(at, 1.0);
    out.position = position_world_to_clip(at);
    out.world_normal = vec3<f32>(0.0, 1.0, 0.0);
    // Where the point lay before the waves moved it, and how far they
    // lifted it.
@if(VERTEX_COLORS)
    out.color = vec4<f32>(flat.xz, displacement.y, 0.0);
@if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = vertex.instance_index;
@if(VISIBILITY_RANGE_DITHER)
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(
        vertex.instance_index,
        world_from_local[3],
    );
    return out;
}

// How much foam is left `age` seconds after the waves left `fresh` of it,
// fading by `decay` a share a second and eaten by `eaten` a second.
fn held(fresh: f32, age: f32, decay: f32, eaten: f32) -> f32 {
    if decay < 1e-4 {
        return max(fresh - eaten * age, 0.0);
    }
    let floor = eaten / decay;
    return max((fresh + floor) * exp(-decay * age) - floor, 0.0);
}

// How much foam lies at the point `pos` metres across, from 0 to 1, worked
// out from the waves' past where it is drawn: the most of what the waves
// left there, as hard as they squeezed the surface together then and faded
// since. They squeeze it hardest as their crests pass, so the past is
// looked at now and as each wave's last two crests passed, with all five
// waves as they were then. Where the noise lets none be born, none is
// looked for, nor in a past that has faded too far to leave more than is
// found already, or than `least`, the least that shows; the waves turned
// to the shore count only where they have turned enough to show. `near` is
// how the shore bears on the waves there, `kept` what each keeps, and
// `allowed` how much the noise lets be born, above zero.
fn worked_foam(pos: vec2<f32>, allowed: f32, least: f32, near: Coast, kept: array<vec2<f32>, 5>) -> f32 {
    let world_uv = pos / 15.0;
    let t = water.time.x;
    let scale = water.time.y;
    let p = pos / scale;
    let downwind = water.wind.xy;
    let streak = water.fading.zw;
    let landward = -near.landward;
    let shoreward = near.turned > 0.05;
    let eaten = textureSampleLevel(noise, noise_sampler, world_uv * 24.0, 0.0).r * water.fading.y;
    // The crests of the waves that hold sway here.
    var times = crests(p, t, vec2<f32>(0.0), 0.0, false, downwind, kept);
    if near.turned > 0.5 {
        times = crests(p, t, landward, near.along / scale, true, downwind, kept);
    }
    var most = 0.0;
    for (var k = 0u; k < 11u; k++) {
        var age = 0.0;
        if k > 0u {
            let crest = times[(k - 1u) / 2u];
            age = crest.x + f32((k - 1u) % 2u) * crest.y;
        }
        if held(min(allowed, 1.0), age, water.fading.x, eaten) <= max(most, least) {
            continue;
        }
        let then = t - age;
        var d = bends(p, then, vec2<f32>(0.0), 0.0, false, downwind, 0.0, kept, 0.0).pull;
        if shoreward {
            let turned = bends(p, then, landward, near.along / scale, true, downwind, near.close, kept, 0.0).pull;
            d = mix(d, turned, near.turned);
        }
        // The drifting noise pushes it by `streak` too, so the foam it
        // leaves lies in streaks, crisp along the crests.
        let g = noise_rise(noise_slope, noise_slope_sampler, p, then, downwind);
        d += vec4<f32>(streak.x * g, streak.y * g);
        d *= near.gain;
        let area = (1.0 + d.x) * (1.0 + d.w) - d.y * d.z;
        let fresh = clamp((1.0 - smoothstep(0.0, 0.7, area)) * allowed, 0.0, 1.0);
        most = max(most, held(fresh, age, water.fading.x, eaten));
    }
    return most;
}

// How far beyond the surface, along the eye's line, what was drawn at the
// screen's point `uv` lies, where its depth is `depth` and the surface is
// `surface_z` from the eye in view space.
fn through_water(uv: vec2<f32>, depth: f32, surface_z: f32) -> f32 {
    return surface_z - depth_ndc_to_view_z(depth);
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
const MIRROR_STEPS: u32 = 24u;
// The fewest pixels one step covers: a reflection on rippled water need not
// be sharp, so it is found at a coarser grain than the screen's.
const MIRROR_GRAIN: f32 = 6.0;
// The halvings a hit is narrowed down by between its last two steps.
const MIRROR_NARROWING: u32 = 3u;

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
//
// The four pixels of each square of two by two walk together: each tests
// only every fourth step, along its own ray, which lies so near theirs that
// what one finds the others would. The first step any of them hits is
// narrowed down by each along its own ray. So each takes a quarter of the
// steps, which, each reading the depth somewhere else on the screen, are
// most of the water's cost. All four must call it, with the same `steps`.
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
    let stride = max(MIRROR_GRAIN, pixels / f32(steps)) / max(length(span), 1e-4);
    // Each pixel starts a little further along than its neighbour, so the
    // steps leave no bands; the ripples hide the grain this leaves.
    let jitter = fract(52.982918 * fract(dot(pixel, vec2<f32>(0.06711056, 0.00583715))));

    // This pixel's share of the steps, and the first of them that hits.
    // A ray too short to cross a pixel tests none.
    let lane = u32(pixel.x) % 2u + 2u * (u32(pixel.y) % 2u);
    var first = steps;
    if pixels >= 1.0 {
        for (var i = lane; i < steps; i += 4u) {
            let along = stride * (jitter + f32(i));
            if along > leaves {
                break;
            }
            let last = select(stride * (jitter + f32(i) - 1.0), 0.0, i == 0u);
            let ray = -1.0 / mix(k0, k1, along);
            let scene = scene_z(mix(s0, s1, along));
            // How far the ray's depth moved in this step: anything thinner
            // than that could be stepped over, so a hit may be that deep.
            let moved = abs(ray + 1.0 / mix(k0, k1, last));
            if ray < scene && ray > scene - (moved + 0.3 + 0.01 * -scene) {
                first = i;
                break;
            }
        }
    }
    var hit = min(first, quadSwapX(first));
    hit = min(hit, quadSwapY(hit));
    let along = stride * (jitter + f32(hit));
    if hit >= steps || pixels < 1.0 || along > leaves {
        return vec4<f32>(0.0);
    }

    var a = select(stride * (jitter + f32(hit) - 1.0), 0.0, hit == 0u);
    var b = along;
    for (var n = 0u; n < MIRROR_NARROWING; n++) {
        let mid = (a + b) * 0.5;
        if -1.0 / mix(k0, k1, mid) < scene_z(mix(s0, s1, mid)) {
            b = mid;
        } else {
            a = mid;
        }
    }
    let at = mix(s0, s1, b);
    // A little blurred: the reflection need not be sharper than the grain
    // it was found at.
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

// 1 above `edge` and 0 below, with an edge a pixel wide wherever it is seen,
// so that foam's edges neither alias nor blur.
fn aa_step(edge: f32, v: f32) -> f32 {
    let width = clamp(fwidth(v), 1e-4, 0.25);
    return clamp((v - edge) / width + 0.5, 0.0, 1.0);
}

// The four points of the land's pages about a point on the map, each the
// height and the distance out from the shore, and where between them the
// point lies.
struct PageCell {
    a: vec2<f32>,
    b: vec2<f32>,
    c: vec2<f32>,
    d: vec2<f32>,
    f: vec2<f32>,
}

// The height and the distance out from the shore at the texel `c` of the
// page of size `level`, as `relief.wesl` packs them.
fn page_texel(level: i32, c: vec2<i32>) -> vec2<f32> {
    let t = textureLoad(pages, c, level, 0).xy;
    return vec2<f32>(bitcast<f32>(t.x), (f32(t.y >> 16u) / 65535.0 * 2.0 - 1.0) * SHORE_REACH);
}

// The points about `p` on the finest of the land's pages from `least` up
// that reaches it, as `page_at` in `ground_shape.wesl` finds them. False
// until the pages are made.
fn page_cell(p: vec2<f32>, least: i32, cell: ptr<function, PageCell>) -> bool {
    let size = vec2<i32>(textureDimensions(pages)) - 2;
    if any(size < vec2<i32>(1)) {
        return false;
    }
    var level = least;
    var g = vec2<f32>(0.0);
    loop {
        let at = pages_at.at[level];
        g = (p - at.xy) / at.z;
        if level >= 7 || (all(g >= vec2<f32>(0.0)) && all(g < vec2<f32>(size))) {
            break;
        }
        level += 1;
    }
    let c = clamp(vec2<i32>(floor(g)), vec2<i32>(0), size);
    *cell = PageCell(
        page_texel(level, c),
        page_texel(level, c + vec2<i32>(1, 0)),
        page_texel(level, c + vec2<i32>(0, 1)),
        page_texel(level, c + vec2<i32>(1, 1)),
        fract(g),
    );
    return true;
}

// The height of the ground under the point `p` on the map, from the land's
// pages, smoothly between their points: from the page whose points lie
// about as far apart as a pixel, `pixel` metres, there, or the next larger
// that reaches it. Finer than the ground may be drawn there, so the water's
// bands and edge follow the river's true banks, not the steps of a coarse
// patch, whose rivers are cut wider. Deep under the water until the pages
// are made.
fn ground_under(p: vec2<f32>, pixel: f32) -> f32 {
    let least = i32(clamp(floor(log2(max(pixel, 1e-3) / pages_at.at[0].z)), 0.0, 7.0));
    var cell: PageCell;
    if !page_cell(p, least, &cell) {
        return pages_at.water.x - 64.0;
    }
    return mix(mix(cell.a.x, cell.b.x, cell.f.x), mix(cell.c.x, cell.d.x, cell.f.x), cell.f.y);
}

// How the shore bears on the waves at the point `p` on the map, from the
// distance out from it the land's pages hold, smoothly between their points
// and from the finest that reaches it, the way to it falling across them.
// Open sea until the pages are made.
fn shore_at(p: vec2<f32>) -> Coast {
    var cell: PageCell;
    if !page_cell(p, 0, &cell) {
        return coast(SHORE_REACH, vec2<f32>(0.0), water.time.z, water.calm.xy);
    }
    let f = cell.f;
    let d = mix(mix(cell.a.y, cell.b.y, f.x), mix(cell.c.y, cell.d.y, f.x), f.y);
    let rise = vec2<f32>(mix(cell.b.y - cell.a.y, cell.d.y - cell.c.y, f.y), mix(cell.c.y - cell.a.y, cell.d.y - cell.b.y, f.x));
    return coast(d, -rise, water.time.z, water.calm.xy);
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
    let carried = vec4<f32>(in.world_position.xz, 0.0, 0.0);
    let world_xz = carried.xy;
    let t = water.time.x;
    let scale = water.time.y;
    let p = world_xz / scale;
    let downwind = water.wind.xy;

    // The foam thins out from `fade.x` to `fade.y` metres from the eye,
    // and the slope fades far off, where the waves are a few pixels long
    // and the water reads as flat.
    let shape = smoothstep(0.4, 1.0, textureSample(sea_foam, sea_foam_sampler, world_xz / 7.5).r);
    let from_eye = distance(view.world_position, in.world_position.xyz);
    let thinned = 1.0 - smoothstep(water.foam.x, max(water.foam.y, water.foam.x + 1.0), from_eye);
    // The foam is born only where the noise lets it.
    let allowed = smoothstep(0.2, 0.7, textureSampleLevel(noise, noise_sampler, world_xz / 15.0, 0.0).r) * 8.0;
    let showing = thinned * shape * f32(allowed > 0.0);
    // The longest wave is gone where a pixel is a quarter of it long, as
    // `bends` fades them, and the others before it.
    let footprint = length(fwidth(world_xz)) / scale;
    let held = f32(footprint < TAU / (SHAPES[0].y * 4.0));
    let detail = (1.0 - smoothstep(SLOPE_FADE.x * scale, SLOPE_FADE.y * scale, from_eye)) * water.calm.w * held;

    // How the shore bears on the waves here, and what the noise gives each
    // wave, read once for the foam and the slope, the noise only where
    // either is wanted.
    let at_shore = shore_at(world_xz);
    var kept: array<vec2<f32>, 5>;
    if showing > 0.0 || detail > 0.0 {
        kept = groups(noise, noise_sampler, p, t, downwind);
    }

    // The foam, worked out from the waves' past and painted in broad
    // shapes, from the coarse foam alone. Past where it has thinned away,
    // or outside the shapes, it is not worked out at all. The rim of the
    // faintest foam drawn begins at a tenth.
    var foam = 0.0;
    if showing > 0.0 {
        foam = clamp(worked_foam(world_xz, allowed, 0.1 / showing, at_shore, kept) * showing, 0.0, 1.0);
    }

    // The slope is found again for each point drawn, from the waves
    // themselves, so that far off, where the mesh lies flat, the water
    // still shows them: exactly, each wave fading as a pixel grows too
    // wide to hold it, so that it does not shimmer. The drifting noise
    // fades as the finest wave does. Where it has faded away it is not
    // found at all.
    var N = vec3<f32>(0.0, 1.0, 0.0);
    if detail > 0.0 {
        var bend = bends(p, t, vec2<f32>(0.0), 0.0, false, downwind, 0.0, kept, footprint);
        if at_shore.turned > 0.0 {
            let turned = bends(p, t, -at_shore.landward, at_shore.along / scale, true, downwind, at_shore.close, kept, footprint);
            bend = Bend(mix(bend.rise, turned.rise, at_shore.turned), mix(bend.pull, turned.pull, at_shore.turned));
        }
        let fine = 1.0 - smoothstep(0.4, 1.8, footprint);
        let lift = 0.05 * fine * noise_rise(noise_slope, noise_slope_sampler, p, t, downwind);
        let by = at_shore.gain * detail;
        let rise = (bend.rise + lift) * by;
        let pull = bend.pull * by;
        let along_x = vec3<f32>(1.0 + pull.x, rise.x, pull.z);
        let along_z = vec3<f32>(pull.y, rise.y, 1.0 + pull.w);
        N = normalize(cross(along_z, along_x));
    }
    // Far off, where the waves are finer than a pixel and have faded, the
    // sea is still rough: `rough` is how much of it they no longer show.
    // Broad, slow swells of the drifting noise tilt it a little there, so
    // the sky it mirrors breaks into soft patches rather than one sheet.
    let rough = max(
        smoothstep(0.5, TAU / (SHAPES[0].y * 4.0), footprint),
        smoothstep(SLOPE_FADE.x * scale, SLOPE_FADE.y * scale, from_eye),
    );
    if rough > 0.0 {
        let swell = textureSampleLevel(noise_slope, noise_slope_sampler, world_xz / FAR_SWELL + t * 0.002, 0.0).rg;
        let tilt = swell * (SLOPE_MOST / 10.0) * FAR_TILT * rough;
        N = normalize(N - vec3<f32>(tilt.x, 0.0, tilt.y));
    }

    // What lies under the surface, bent by its slope as the eye sees it,
    // less so far off and where the water is thin, and how far beyond the
    // surface it lies. Where the bent point falls on nothing, the sky,
    // nothing shows through.
    var under = vec3<f32>(0.0);
    var through = 64.0;
    // How deep the water stands over the ground below, straight down, the
    // same however it is looked at. Near the eye, from what was drawn
    // under it, which holds all that stands in the water; far off, where the
    // ground is drawn in large flat triangles that would show in the bands
    // and the strip of foam it lays, from the land's pages, smoothly between
    // their points; between, the one blending into the other.
    var below = max(in.world_position.y - ground_under(world_xz, footprint * scale), 0.0);
    let far = smoothstep(DEPTH_FROM_PAGES.x, DEPTH_FROM_PAGES.y, from_eye);
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
        if far < 1.0 {
            let clip = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, depth, 1.0);
            let bed = view.world_from_clip * clip;
            below = mix(max(in.world_position.y - bed.y / bed.w, 0.0), below, far);
        }
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
        // Crisp where the waves show; where the sea is rough past what can
        // be seen, spread into a broad, dim sheen, as the waves would
        // scatter it, rather than one blinding patch.
        let crisp = smoothstep(0.993, 0.996, dot(N, H)) * 2.5;
        let sheen = pow(smoothstep(0.9, 1.0, dot(N, H)), 2.0) * 0.25;
        glint += light_in * mix(crisp, sheen, rough);
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
    // A square of two by two pixels walks its rays together, so it
    // decides together, as the one of them whose reflection shows most.
    var most = max(weight, quadSwapX(weight));
    most = max(most, quadSwapY(most));
    if most > 0.01 {
        // A faint reflection is found in fewer, coarser steps: what it
        // misses is hidden in how little of it shows.
        let steps = u32(mix(8.0, f32(MIRROR_STEPS), smoothstep(0.02, 0.3, most)));
        let hit = mirror_hit(in.world_position.xyz, R, in.position.xy, steps);
        mirrored = mix(mirrored, hit.rgb, hit.a);
    }
}
    colour = mix(colour, mirrored, weight);

    // Foam in crisp, solid shapes with a darker rim, drawn from how much of
    // it there is.
    let froth = max(foam, strip);
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
