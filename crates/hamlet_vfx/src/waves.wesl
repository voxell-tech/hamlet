// The water's waves, which move its surface, and from whose past its foam is
// worked out, where they squeezed the surface together. Ported from an
// MIT-licensed water shader; its notice is in `water/LICENSE`.
//
// Five Gerstner waves of set heading, height, length and speed, each
// roughened by a drifting noise. Out at sea they keep their headings; as the
// water nears land they turn to run in to it, as shallowing water turns
// waves: within `bend` metres of the shore the same five waves, running
// straight in, take over by degrees. Positions and heights are in units
// `scale` metres long, so the whole sea can be made larger or smaller at
// once.
//
// What the noise gives each wave at a point is read once, by `groups`, and
// shared by the waves out at sea and those turned to the shore, by the
// surface's place, its slope and its foam. The waves' slopes are exact, from
// `bends`, with no texture read.

// The farthest from the shore its distance tells, in metres. `SHORE_REACH`
// in `water.rs`.
const SHORE_REACH: f32 = 16.0;
const TAU: f32 = 6.28318531;
// The steepest slope the noise's slope picture holds, which it is read back
// by. `SLOPE_MOST` in `water.rs`.
const SLOPE_MOST: f32 = 64.0;

// The way each wave runs, the first downwind and the rest about it, in the
// wind's frame.
const WAYS = array<vec2<f32>, 5>(
    vec2<f32>(0.0, 1.0),
    vec2<f32>(0.70710678, 0.70710678),
    vec2<f32>(0.4472136, 0.8944272),
    vec2<f32>(-0.3713907, 0.9284767),
    vec2<f32>(-0.2747211, 0.9615239),
);
// Height, frequency, speed and steepness of each. Long waves run fast, as
// at sea, and each is as sharp at its crest for its length.
const SHAPES = array<vec4<f32>, 5>(
    vec4<f32>(0.4, 0.625, 1.03, 0.32),
    vec4<f32>(0.15, 0.81, 1.26, 0.19),
    vec4<f32>(0.12, 1.25, 1.58, 0.27),
    vec4<f32>(0.01, 1.875, 2.37, 0.16),
    vec4<f32>(0.025, 2.5, 3.16, 0.13),
);

// Turns the wind's frame, where the wind blows toward -z, to the world's,
// where it blows `downwind`. A wave runs against the way its phase grows.
fn to_wind(downwind: vec2<f32>) -> mat2x2<f32> {
    let u = -downwind;
    return mat2x2<f32>(vec2<f32>(u.y, -u.x), u);
}

// How much of its full height the wave `i` keeps at `p` at time `t`, and how
// far its phase is turned there: each its own slow patches of noise tens of
// metres across, drifting along `way`.
//
// Every noise drifts the way its wave runs out at sea, even where the wave
// has turned to the shore: the turned waves run another way at every point
// about the coast, and a noise drifted by them for all the time gone by
// would be torn apart along the coast into streaks running out from it.
fn varied(
    noise: texture_2d<f32>,
    noise_sampler: sampler,
    p: vec2<f32>,
    t: f32,
    way: vec2<f32>,
    i: u32,
) -> vec2<f32> {
    let seed = vec2<f32>(0.37, 0.71) * f32(i + 1u);
    let group = textureSampleLevel(noise, noise_sampler, p * 0.012 + seed - way * t * 0.004, 0.0).r;
    let wobble = textureSampleLevel(noise, noise_sampler, p * 0.008 + seed.yx + 0.5, 0.0).r;
    return vec2<f32>(mix(0.25, 1.15, smoothstep(0.15, 0.85, group)), (wobble - 0.5) * 6.0);
}

// How much of its height and how far its phase is turned each of the five
// waves keeps at `p` at time `t`, as `varied` has it, turned or not.
fn groups(
    noise: texture_2d<f32>,
    noise_sampler: sampler,
    p: vec2<f32>,
    t: f32,
    downwind: vec2<f32>,
) -> array<vec2<f32>, 5> {
    let turned_to = to_wind(downwind);
    let ways = WAYS;
    var out: array<vec2<f32>, 5>;
    for (var i = 0u; i < 5u; i++) {
        out[i] = varied(noise, noise_sampler, p, t, turned_to * ways[i], i);
    }
    return out;
}

// The five waves at `p` at time `t`, keeping `kept` of their height and
// phase, each turned to `turn` by as much as `bent`: where it is whole, each
// runs along `turn`, `along` the distance along it. Without the noise's
// lift, which is the same turned or not.
fn five(
    p: vec2<f32>,
    t: f32,
    turn: vec2<f32>,
    along: f32,
    bent: bool,
    downwind: vec2<f32>,
    close: f32,
    kept: array<vec2<f32>, 5>,
) -> vec3<f32> {
    let turned_to = to_wind(downwind);
    let ways = WAYS;
    let shapes = SHAPES;
    var by = kept;
    var d = vec3<f32>(0.0);
    for (var i = 0u; i < 5u; i++) {
        let open = turned_to * ways[i];
        let way = select(open, turn, bent);
        let distance = select(dot(open, p), along, bent);
        let s = shapes[i];
        let v = by[i];
        // Turned to the shore, they rise and steepen as the water
        // shallows, `close` to it, then break. Further out they draw the
        // water toward their crests less: running in from every side of
        // a rock, they would squeeze all the water about it together.
        let rise = select(1.0, 1.0 + 0.6 * close, bent);
        let steep = select(s.w, s.w * mix(0.5, 1.0, close), bent);
        let phase = t * s.z + distance * s.y + v.y;
        let push = way * cos(phase) * steep * v.x;
        d += vec3<f32>(push.x, sin(phase) * s.x * rise * v.x, push.y);
    }
    return d;
}

// How far the drifting noise lifts the water at `p` at time `t`: a little
// under each wave, drifting the way it runs out at sea, and a broad, slow
// lift as well, so the surface is never the same twice.
fn lifted(
    noise: texture_2d<f32>,
    noise_sampler: sampler,
    p: vec2<f32>,
    t: f32,
    downwind: vec2<f32>,
) -> f32 {
    let turned_to = to_wind(downwind);
    let ways = WAYS;
    var y = 0.0;
    for (var i = 0u; i < 5u; i++) {
        let lift = textureSampleLevel(noise, noise_sampler, p * 0.2 + t * (turned_to * ways[i]) * 0.07, 0.0).r;
        y += lift * 0.05 - 0.025;
    }
    let broad = textureSampleLevel(noise, noise_sampler, p * 0.04 + vec2<f32>(t * 0.01, -t * 0.013), 0.0).r;
    return y + broad * 0.08 - 0.04;
}

// How far the waves move the point `pos` metres across at time `t`, where
// the shore bears on them as `near` has it and the wind blows `downwind`.
fn wave(
    noise: texture_2d<f32>,
    noise_sampler: sampler,
    near: Coast,
    pos: vec2<f32>,
    t: f32,
    scale: f32,
    downwind: vec2<f32>,
) -> vec3<f32> {
    let p = pos / scale;
    let kept = groups(noise, noise_sampler, p, t, downwind);
    var d = five(p, t, vec2<f32>(0.0), 0.0, false, downwind, 0.0, kept);
    if near.turned > 0.0 {
        let shoreward = five(p, t, -near.landward, near.along / scale, true, downwind, near.close, kept);
        d = mix(d, shoreward, near.turned);
    }
    d.y += lifted(noise, noise_sampler, p, t, downwind);
    return d * near.gain * scale;
}

// How the shore bears on the waves at a point.
struct Coast {
    // The share of their height they keep there.
    gain: f32,
    // How far they have turned to run in to the land, from 0 to 1.
    turned: f32,
    // How near the land they are, from 0 at `bend` to 1 at its edge.
    close: f32,
    // The metres to the shore, and the way to it.
    along: f32,
    landward: vec2<f32>,
}

// How the shore bears on the waves at a point `d` metres out to sea from
// it, below zero inland, the way to it rising along `toward`. They turn to
// it within `bend` metres of it, all the way by halfway in, and within
// `calm.x` metres of it die down, to `calm.y` of their height at the water's
// edge. Water well inland, a river's or a pool's, is not the sea's: there
// they do not turn.
fn coast(d: f32, toward: vec2<f32>, bend: f32, calm: vec2<f32>) -> Coast {
    let gain = mix(calm.y, 1.0, smoothstep(0.0, max(calm.x, 1e-3), d));
    // Only the sea's waves turn to the land. A river is narrow, and its
    // waves would turn to both banks at once and meet down its middle.
    let sea = smoothstep(-6.0, -2.0, d);
    var turned = (1.0 - smoothstep(bend * 0.55, max(bend, 1e-3), d)) * sea;
    var landward = vec2<f32>(0.0, 1.0);
    if dot(toward, toward) < 1e-6 {
        turned = 0.0;
    } else {
        landward = normalize(toward);
    }
    // Running in to the land, the phase falls toward the shore, so each
    // crest comes in from the sea.
    let close = 1.0 - clamp(d / max(bend, 1e-3), 0.0, 1.0);
    return Coast(gain, turned, close, d, landward);
}

// How the waves tilt and draw together the surface about a point.
struct Bend {
    // How steeply they lift it as the point moves along x and along z.
    rise: vec2<f32>,
    // How much further they push it along x and along z as the point
    // moves along x and along z, as x by x, x by z, z by x and z by z.
    pull: vec4<f32>,
}

// How the five waves, laid as `five` lays them and keeping `kept`, bend the
// surface about `p` at time `t`, exactly, as their slopes have it. A wave
// fewer than eight of `footprint` long, the width a pixel covers there,
// fades, and is gone at four: shorter, it would only shimmer.
fn bends(
    p: vec2<f32>,
    t: f32,
    turn: vec2<f32>,
    along: f32,
    bent: bool,
    downwind: vec2<f32>,
    close: f32,
    kept: array<vec2<f32>, 5>,
    footprint: f32,
) -> Bend {
    let turned_to = to_wind(downwind);
    let ways = WAYS;
    let shapes = SHAPES;
    var by = kept;
    var out = Bend(vec2<f32>(0.0), vec4<f32>(0.0));
    for (var i = 0u; i < 5u; i++) {
        let open = turned_to * ways[i];
        let way = select(open, turn, bent);
        let distance = select(dot(open, p), along, bent);
        let s = shapes[i];
        let v = by[i];
        let rise = select(1.0, 1.0 + 0.6 * close, bent);
        let steep = select(s.w, s.w * mix(0.5, 1.0, close), bent);
        let phase = t * s.z + distance * s.y + v.y;
        let pixels = TAU / (s.y * max(footprint, 1e-6));
        let seen = v.x * s.y * smoothstep(4.0, 8.0, pixels);
        out.rise += way * (cos(phase) * s.x * rise * seen);
        let pinch = steep * seen * sin(phase);
        out.pull -= pinch * vec4<f32>(way.x * way.x, way.x * way.y, way.y * way.x, way.y * way.y);
    }
    return out;
}

// How steeply the drifting noise under the five waves rises about `p` at
// time `t`, as `lifted` lays it, over x and z, from the noise's slope
// picture `slope`, found a few of its texels wide.
fn noise_rise(
    slope: texture_2d<f32>,
    slope_sampler: sampler,
    p: vec2<f32>,
    t: f32,
    downwind: vec2<f32>,
) -> vec2<f32> {
    let turned_to = to_wind(downwind);
    let ways = WAYS;
    var g = vec2<f32>(0.0);
    for (var i = 0u; i < 5u; i++) {
        let uv = p * 0.2 + t * (turned_to * ways[i]) * 0.07;
        g += textureSampleLevel(slope, slope_sampler, uv, 0.0).rg;
    }
    return g * SLOPE_MOST * 0.2;
}

// For each of the five waves, laid as `five` lays them and keeping `kept`
// of their phase, the seconds since its crest last passed `p` at time `t`,
// and the seconds between one crest and the next.
fn crests(
    p: vec2<f32>,
    t: f32,
    turn: vec2<f32>,
    along: f32,
    bent: bool,
    downwind: vec2<f32>,
    kept: array<vec2<f32>, 5>,
) -> array<vec2<f32>, 5> {
    let turned_to = to_wind(downwind);
    let ways = WAYS;
    let shapes = SHAPES;
    var by = kept;
    var out: array<vec2<f32>, 5>;
    for (var i = 0u; i < 5u; i++) {
        let distance = select(dot(turned_to * ways[i], p), along, bent);
        let s = shapes[i];
        // Its crest stands where the phase is a quarter turn.
        let past = t * s.z + distance * s.y + by[i].y - TAU * 0.25;
        out[i] = vec2<f32>((past - TAU * floor(past / TAU)) / s.z, TAU / s.z);
    }
    return out;
}
