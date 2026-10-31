// A page of the land's heights, one per texel, as `Land::height_wide` in
// `land/terrain.rs` has them: rolling plains, flat-topped hills, town
// sites levelled, rivers cut, the island's coast falling to the sea floor
// and held ground kept level. The two must give the same land, so a
// change to one is a change to the other.

import hamlet_vfx::noise::{fbm, key1, perlin, scramble, smooth_step};

// The fields are `ReliefInput`'s in `relief.rs`, where each is explained.
struct Relief {
    page: vec4<f32>,
    plains: vec4<f32>,
    rise: vec4<f32>,
    mounds: vec4<f32>,
    tops: vec4<f32>,
    sizes: vec4<f32>,
    sea: vec4<f32>,
    rim: vec4<f32>,
    stacks: vec4<f32>,
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

// `stroke` in `terrain.rs`.
fn stroke(dist: f32, flat: f32, run: f32) -> f32 {
    let u = max(dist - flat, 0.0) / max(run, 0.01);
    return exp(-5.5 * u * u);
}

// `segment_dist` in `terrain.rs`.
fn segment_dist(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
    let ab = b - a;
    let t = clamp(dot(p - a, ab) / max(dot(ab, ab), 1e-6), 0.0, 1.0);
    return distance(p, a + ab * t);
}

// `ragged` in `terrain.rs`.
fn ragged(p: vec2<f32>) -> f32 {
    return relief.mounds.w
        * (0.7 * perlin(p / 5.0, seed() + 58u) + 0.3 * perlin(p / 2.0, seed() + 59u));
}

// `near_town` in `terrain.rs`.
fn near_town(p: vec2<f32>, reach: f32) -> bool {
    let clear = reach + relief.town.y;
    let first = relief.counts.y;
    for (var i = first; i < first + relief.counts.z; i++) {
        if distance(relief.points[i].xy, p) < clear {
            return true;
        }
    }
    return false;
}

// `hill` in `terrain.rs`.
fn hill(p: vec2<f32>) -> f32 {
    let side = relief.mounds.x;
    let share = relief.mounds.y;
    let edge = length(p) + relief.rise.w * perlin(p * relief.rise.z, seed() + 50u);
    if edge < relief.rise.y {
        return 0.0;
    }
    let cell = floor(p / side);
    let rag = ragged(p);
    var top = 0.0;
    let s = seed() + 55u;
    for (var dz = -1; dz <= 1; dz++) {
        for (var dx = -1; dx <= 1; dx++) {
            let xi = i32(cell.x) + dx;
            let zi = i32(cell.y) + dz;
            let x = bitcast<u32>(xi);
            let z = bitcast<u32>(zi);
            if unit_of(s, x, z, 0u) > share {
                continue;
            }
            let mid = (vec2<f32>(f32(xi), f32(zi)) + 0.2
                + vec2<f32>(unit_of(s, x, z, 1u), unit_of(s, x, z, 2u)) * 0.6) * side;
            let a = unit_of(s, x, z, 3u) * 6.2831855;
            let half = vec2<f32>(cos(a), sin(a)) * relief.mounds.z * unit_of(s, x, z, 4u) * 0.5;
            let tall = mix(relief.tops.x, relief.tops.y, unit_of(s, x, z, 5u));
            let flat = mix(relief.sizes.x, relief.sizes.y, unit_of(s, x, z, 6u));
            let skirt = radians(mix(relief.sizes.z, relief.sizes.w, unit_of(s, x, z, 7u)));
            let run = tall / max(tan(skirt), 0.05);
            if near_town(mid, flat + run + length(half) + 8.0) {
                continue;
            }
            let along = segment_dist(p, mid - half, mid + half);
            top = max(top, tall * stroke(along + rag, flat, run));
        }
    }
    if top <= 0.0 {
        return 0.0;
    }
    let lumps = 0.35 * perlin(p / 16.0, seed() + 56u) + 0.15 * perlin(p / 8.0, seed() + 57u);
    return top + lumps * min(top / max(relief.tops.y, 0.1), 1.0);
}

// `wild` in `terrain.rs`.
fn wild(p: vec2<f32>, d: f32) -> f32 {
    let h = relief.plains.x
        + relief.plains.z * fbm(p * relief.plains.y, 4u, seed() + 20u)
        + relief.rise.x * perlin(p * relief.plains.w, seed() + 30u);
    return h + hill(p) * smooth_step(8.0, 40.0, d);
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

// `inside` in `terrain.rs`.
fn inside(p: vec2<f32>) -> f32 {
    let island = relief.tops.z;
    var near = FAR;
    let first = relief.counts.y;
    for (var i = first; i < first + relief.counts.z; i++) {
        near = smooth_min(near, distance(relief.points[i].xy, p), island * 0.3);
    }
    return island - near - relief.sea.y * perlin(p * relief.sea.x, seed() + 40u);
}

// `unit(key(&[a, b, c, d]))` in `noise.rs`.
fn unit_of(a: u32, b: u32, c: u32, d: u32) -> f32 {
    var h = scramble(0x51ed270bu ^ a);
    h = scramble(h ^ b);
    h = scramble(h ^ c);
    h = scramble(h ^ d);
    return f32(h >> 8u) * (1.0 / 16777216.0);
}

// `stack` in `terrain.rs`.
fn stack(p: vec2<f32>, bed: f32) -> f32 {
    let side = relief.stacks.x;
    let share = relief.stacks.y;
    let tall = relief.stacks.z;
    let cell = floor(p / side);
    var top = -FAR;
    let s = seed() + 43u;
    for (var dz = -1; dz <= 1; dz++) {
        for (var dx = -1; dx <= 1; dx++) {
            let xi = i32(cell.x) + dx;
            let zi = i32(cell.y) + dz;
            let x = bitcast<u32>(xi);
            let z = bitcast<u32>(zi);
            if unit_of(s, x, z, 0u) > share {
                continue;
            }
            let at = (vec2<f32>(f32(xi), f32(zi)) + 0.2
                + vec2<f32>(unit_of(s, x, z, 1u), unit_of(s, x, z, 2u)) * 0.6) * side;
            let off = inside(at);
            if off < -24.0 || off > -5.0 {
                continue;
            }
            let flat = 0.8 + 1.4 * unit_of(s, x, z, 3u);
            let height = relief.sea.w + tall * (0.4 + 0.6 * unit_of(s, x, z, 4u));
            let rise = stroke(distance(at, p), flat, 2.5 + 1.5 * unit_of(s, x, z, 5u));
            top = max(top, bed + (height - bed) * rise);
        }
    }
    return top;
}

// `shore` in `terrain.rs`.
fn shore(p: vec2<f32>, h: f32) -> f32 {
    let water = relief.sea.w;
    let into = inside(p) + ragged(p);
    let run = mix(relief.rim.y, relief.rim.z, 0.5 + 0.5 * perlin(p * 0.012, seed() + 41u));
    let bed = mix(water - relief.rim.x, water - relief.sea.z, smooth_step(0.0, -40.0, into));
    let ground = bed + (h - bed) * stroke(run - into, 0.0, run);
    if into >= -30.0 && into < run {
        return max(ground, stack(p, bed));
    }
    return ground;
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
    h = shore(p, h);

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
