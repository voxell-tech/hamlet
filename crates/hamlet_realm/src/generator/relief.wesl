// A page of the land, one texel to a point of a grid, as `land/terrain.rs`
// has it: its height as `Land::height_wide` has it, with rolling plains,
// flat-topped hills, town sites levelled, rivers cut, each island's coast
// falling to the sea floor and held ground kept level; and its share of
// woods, as `forest` has it, and of heath. The two must give the same land,
// so a change to one is a change to the other.

import hamlet_vfx::noise::{fbm, key1, perlin, scramble, smooth_step};

// The fields are `ReliefInput`'s in `relief.rs`, where each is explained.
struct Relief {
    plains: vec4<f32>,
    rise: vec4<f32>,
    mounds: vec4<f32>,
    tops: vec4<f32>,
    sizes: vec4<f32>,
    sea: vec4<f32>,
    rim: vec4<f32>,
    town: vec4<f32>,
    river: vec4<f32>,
    drift: vec4<f32>,
    wilds: vec4<f32>,
    counts: vec4<u32>,
    held: vec4<u32>,
    centres: vec4<f32>,
    // Sites, then the shores of the city island and of the hamlet island,
    // as x and z; then held ground as x, z, radius and the height it is held
    // at.
    points: array<vec4<f32>>,
}

// `LevelInput` in `relief.rs`: the page's corner on the map, the width of
// its texel, and how much wider its rivers are drawn.
struct Level {
    page: vec4<f32>,
}

@group(0) @binding(0) var<storage, read> relief: Relief;
@group(0) @binding(1) var<uniform> level: Level;
@group(0) @binding(2) var page: texture_storage_2d<rg32uint, write>;

const FAR: f32 = 1e30;
// `HOLD_EASE` in `land.rs`.
const HOLD_EASE: f32 = 7.0;
// `OPEN` in `terrain.rs`.
const OPEN: f32 = 40.0;
// The farthest from the shore its distance tells, in metres. `SHORE_REACH`
// in `hamlet_vfx`'s `water.rs`.
const SHORE_REACH: f32 = 40.0;

// `Frame` in `land.rs`: the island a point is on, its place measured from
// that island's middle, and how far inside the coast it lies.
struct Frame {
    hamlet: bool,
    local: vec2<f32>,
    inside: f32,
}

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
    let first = relief.counts.y + relief.counts.z;
    for (var i = first; i < first + relief.counts.w; i++) {
        if distance(relief.points[i].xy, p) < clear {
            return true;
        }
    }
    return false;
}

// `hill_in` in `terrain.rs`, with `p` measured from the hamlet island's
// middle.
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
            if near_town(mid + relief.centres.zw, flat + run + length(half) + 8.0) {
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
fn wild(f: Frame, d: f32) -> f32 {
    let p = f.local;
    let h = relief.plains.x
        + relief.plains.z * fbm(p * relief.plains.y, 4u, seed() + 20u)
        + relief.rise.x * perlin(p * relief.plains.w, seed() + 30u);
    if !f.hamlet {
        return h;
    }
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

// `river_dist_in` in `terrain.rs`: only the hamlet island has rivers.
fn river_dist(f: Frame) -> f32 {
    if !f.hamlet {
        return FAR;
    }
    let p = f.local;
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

// How near `p` lies to the `count` shores from `first`, their reaches run
// smoothly together.
fn near_shores(first: u32, count: u32, p: vec2<f32>) -> f32 {
    let island = relief.tops.z;
    var near = FAR;
    for (var i = first; i < first + count; i++) {
        near = smooth_min(near, distance(relief.points[i].xy, p), island * 0.3);
    }
    return near;
}

// `frame` in `terrain.rs`.
fn frame_of(p: vec2<f32>) -> Frame {
    let island = relief.tops.z;
    let city = near_shores(relief.counts.y, relief.counts.z, p);
    let hamlet = near_shores(relief.counts.y + relief.counts.z, relief.counts.w, p);
    let on_hamlet = !(city < hamlet);
    let local = p - select(relief.centres.xy, relief.centres.zw, on_hamlet);
    let near = select(city, hamlet, on_hamlet);
    let inside = island - near - relief.sea.y * perlin(local * relief.sea.x, seed() + 40u);
    return Frame(on_hamlet, local, inside);
}

// `unit(key(&[a, b, c, d]))` in `noise.rs`.
fn unit_of(a: u32, b: u32, c: u32, d: u32) -> f32 {
    var h = scramble(0x51ed270bu ^ a);
    h = scramble(h ^ b);
    h = scramble(h ^ c);
    h = scramble(h ^ d);
    return f32(h >> 8u) * (1.0 / 16777216.0);
}

// How far out from `stroke`'s flat top, as a share of its skirt, the ground
// it lays falls through the water, where it falls from `top` to `bed`.
fn waterline(top: f32, bed: f32) -> f32 {
    let fall = (relief.sea.w - bed) / max(top - bed, 1e-3);
    return sqrt(-log(clamp(fall, 1e-4, 1.0)) / 5.5);
}

// How far `p` lies out to sea from the nearest waterline, in metres, below
// zero on land, at most `SHORE_REACH` either way, where an island's
// skirt falls through the water. It is found from how far inside its
// coast `p` lies, which grows about a metre to a metre, so it runs
// smooth; the coast's raggedness is left out, and the plains taken as
// level.
fn shore_dist(f: Frame) -> f32 {
    let run = mix(relief.rim.y, relief.rim.z, 0.5 + 0.5 * perlin(f.local * 0.012, seed() + 41u));
    let shelf = relief.sea.w - relief.rim.x;
    let d = run * (1.0 - waterline(relief.plains.x, shelf)) - f.inside;
    return clamp(d, -SHORE_REACH, SHORE_REACH);
}

// `shore` in `terrain.rs`.
fn shore(f: Frame, h: f32) -> f32 {
    let water = relief.sea.w;
    let into = f.inside + ragged(f.local);
    let run = mix(relief.rim.y, relief.rim.z, 0.5 + 0.5 * perlin(f.local * 0.012, seed() + 41u));
    let bed = mix(water - relief.rim.x, water - relief.sea.z, smooth_step(0.0, -OPEN, into));
    return bed + (h - bed) * stroke(run - into, 0.0, run);
}

// `height_wide` in `terrain.rs`.
fn height(p: vec2<f32>, f: Frame, wide: f32) -> f32 {
    let d = river_dist(f);
    var h = wild(f, d);

    let site_y = relief.town.x;
    let radius = relief.town.y;
    let ds = site_dist(p);
    let gentle = smooth_step(radius + 15.0, radius, ds);
    h = mix(h, site_y + 0.2 * (h - site_y), gentle);
    h = mix(h, site_y, smooth_step(13.0, 8.0, ds));

    let dw = max(d - wide, 0.0);
    h = mix(0.3, h, smooth_step(4.0, 22.0, dw));
    h = mix(h, -1.3, smooth_step(5.2, 2.2, dw));
    h = shore(f, h);

    let first = relief.counts.y + relief.counts.z + relief.counts.w;
    for (var i = first; i < first + relief.held.x; i++) {
        let hold = relief.points[i];
        let away = distance(hold.xy, p);
        h = mix(h, hold.w, smooth_step(hold.z + HOLD_EASE, hold.z, away));
    }
    return h;
}

// `afar` in `terrain.rs`.
fn afar(f: Frame) -> f32 {
    if !f.hamlet {
        return 0.0;
    }
    let plains = relief.rise.y;
    return smooth_step(plains, plains + 500.0, length(f.local));
}

// `forest` in `terrain.rs`.
fn forest(p: vec2<f32>, f: Frame) -> f32 {
    let q = f.local;
    let n = fbm(q * 0.017, 3u, seed() + 70u);
    let ds = site_dist(p);
    let far_out = afar(f);
    var bare = 0.0;
    if far_out > 0.0 {
        bare = relief.wilds.x * far_out * fbm(q * 0.0021, 2u, seed() + 72u);
    }
    let radius = relief.town.y;
    return smooth_step(-0.12 + bare, 0.22 + bare, n)
        * smooth_step(radius + 2.0, radius + 12.0, ds);
}

// The share of heath in the grass at a point: none near the middle, and far
// out in some regions. Only the ground's colour needs it, so the CPU has none.
fn heath(f: Frame) -> f32 {
    let far_out = afar(f);
    if far_out <= 0.0 {
        return 0.0;
    }
    let share = fbm(f.local * 0.0016, 2u, seed() + 73u);
    return relief.wilds.y * far_out * smooth_step(0.05, 0.4, share);
}

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(page);
    if id.x >= size.x || id.y >= size.y {
        return;
    }
    // Texels lie on the grid's points, the first at the page's corner.
    let p = level.page.xy + vec2<f32>(id.xy) * level.page.z;
    let f = frame_of(p);
    // The height as its bits; then the woods and the heath as two shares of
    // 8 bits, and the distance out from the shore over `SHORE_REACH` either
    // way in 16. Half what four floats would take, and the height exact.
    let shares = pack4x8unorm(vec4<f32>(forest(p, f), heath(f), 0.0, 0.0)) & 0xffffu;
    let out = shore_dist(f) / SHORE_REACH * 0.5 + 0.5;
    let packed = shares | (u32(round(out * 65535.0)) << 16u);
    textureStore(page, id.xy, vec4<u32>(bitcast<u32>(height(p, f, level.page.w)), packed, 0u, 0u));
}
