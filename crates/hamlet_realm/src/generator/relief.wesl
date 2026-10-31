// A page of the land's heights, one per texel, as `Land::height_wide` in
// `land/terrain.rs` has them: rolling plains, flat-topped hills, town
// sites levelled, rivers cut, the island's coast falling to the sea floor
// and held ground kept level. The two must give the same land, so a
// change to one is a change to the other.

import hamlet_vfx::noise::{fbm, key1, perlin, smooth_step};

// The fields are `ReliefInput`'s in `relief.rs`, where each is explained.
struct Relief {
    page: vec4<f32>,
    plains: vec4<f32>,
    rise: vec4<f32>,
    mounds: vec4<f32>,
    tops: vec4<f32>,
    sea: vec4<f32>,
    town: vec4<f32>,
    river: vec4<f32>,
    drift: vec4<f32>,
    counts: vec4<u32>,
    // Sites, then shores, as x and z; then held ground as x, z, radius
    // and the height it is held at.
    points: array<vec4<f32>>,
}

@group(0) @binding(0) var<storage, read> relief: Relief;
@group(0) @binding(1) var heights: texture_storage_2d<r32float, write>;

const FAR: f32 = 1e30;
// `HOLD_EASE` in `land.rs`.
const HOLD_EASE: f32 = 7.0;

fn seed() -> u32 {
    return relief.counts.x;
}

// `mound` in `terrain.rs`.
fn mound(p: vec2<f32>) -> f32 {
    let edge = length(p) + relief.rise.w * perlin(p * relief.rise.z, seed() + 50u);
    let out = smooth_step(relief.rise.y, relief.rise.y + 105.0, edge);
    if out <= 0.0 {
        return 0.0;
    }
    let n = fbm(p * relief.mounds.x, 3u, seed() + 53u);
    return out * smooth_step(relief.mounds.y, relief.mounds.z, n);
}

// `wild` in `terrain.rs`.
fn wild(p: vec2<f32>, d: f32) -> f32 {
    var h = relief.plains.x
        + relief.plains.z * fbm(p * relief.plains.y, 4u, seed() + 20u)
        + relief.rise.x * perlin(p * relief.plains.w, seed() + 30u);
    let m = mound(p);
    if m > 0.0 {
        let tall = 0.5 + 0.5 * perlin(p * relief.mounds.x, seed() + 54u);
        h += m * mix(relief.tops.x, relief.tops.y, tall) * smooth_step(8.0, 40.0, d);
    }
    return h;
}

// `river_centre` in `terrain.rs`.
fn river_centre(k: i32, u: f32) -> f32 {
    let own = seed() + key1(bitcast<u32>(k));
    let s = select(own, seed(), k == 0);
    var drift = 0.0;
    if k != 0 {
        drift = relief.drift.z * perlin(vec2<f32>(u * relief.drift.y, 5.7), own + 1u);
    }
    return 24.0 + f32(k) * relief.river.x + drift
        + relief.river.z * perlin(vec2<f32>(u * relief.river.y, 3.3), s)
        + relief.drift.x * perlin(vec2<f32>(u * relief.river.w, 9.1), s);
}

// `river_dist` in `terrain.rs`.
fn river_dist(p: vec2<f32>) -> f32 {
    let dir = relief.town.zw;
    let u = dot(p, dir);
    let v = dot(p, vec2<f32>(-dir.y, dir.x));
    let gap = relief.river.x;
    let reach = relief.river.z + relief.drift.x + relief.drift.z + 90.0;
    let k0 = i32(round((v - 24.0) / gap));
    var best = FAR;
    var near = FAR;
    for (var k = k0 - 1; k <= k0 + 1; k++) {
        if abs(v - 24.0 - f32(k) * gap) >= reach {
            continue;
        }
        let off = v - river_centre(k, u);
        if abs(off) < abs(best) {
            best = off;
            let slope = river_centre(k, u + 0.5) - river_centre(k, u - 0.5);
            near = abs(off) / sqrt(1.0 + slope * slope);
        }
    }
    return near;
}

// `site_dist` in `land.rs`.
fn site_dist(p: vec2<f32>) -> f32 {
    var near = FAR;
    for (var i = 0u; i < relief.counts.y; i++) {
        near = min(near, distance(relief.points[i].xy, p));
    }
    return near;
}

// `smooth_min` in `terrain.rs`.
fn smooth_min(a: f32, b: f32, k: f32) -> f32 {
    let h = clamp(0.5 + 0.5 * (b - a) / k, 0.0, 1.0);
    return mix(b, a, h) - k * h * (1.0 - h);
}

// `inland` in `terrain.rs`.
fn inland(p: vec2<f32>) -> f32 {
    let island = relief.tops.z;
    var near = FAR;
    let first = relief.counts.y;
    for (var i = first; i < first + relief.counts.z; i++) {
        near = smooth_min(near, distance(relief.points[i].xy, p), island * 0.3);
    }
    let coast = near - island + relief.sea.y * perlin(p * relief.sea.x, seed() + 40u);
    return smooth_step(relief.tops.w, -relief.tops.w, coast);
}

// `height_wide` in `terrain.rs`.
fn height(p: vec2<f32>, wide: f32) -> f32 {
    let d = river_dist(p);
    var h = wild(p, d);

    let site_y = relief.town.x;
    let radius = relief.town.y;
    let ds = site_dist(p);
    let gentle = smooth_step(radius + 15.0, radius, ds);
    h = mix(h, site_y + 0.2 * (h - site_y), gentle);
    h = mix(h, site_y, smooth_step(13.0, 8.0, ds));

    let dw = max(d - wide, 0.0);
    h = mix(0.3, h, smooth_step(4.0, 22.0, dw));
    h = mix(h, -1.3, smooth_step(5.2, 2.2, dw));
    let floor = relief.sea.w - relief.sea.z;
    h = mix(floor, h, inland(p));

    let first = relief.counts.y + relief.counts.z;
    for (var i = first; i < first + relief.counts.w; i++) {
        let hold = relief.points[i];
        let away = distance(hold.xy, p);
        h = mix(h, hold.w, smooth_step(hold.z + HOLD_EASE, hold.z, away));
    }
    return h;
}

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(heights);
    if id.x >= size.x || id.y >= size.y {
        return;
    }
    let p = relief.page.xy + (vec2<f32>(id.xy) + 0.5) * relief.page.z;
    textureStore(heights, id.xy, vec4<f32>(height(p, relief.page.w), 0.0, 0.0, 1.0));
}
