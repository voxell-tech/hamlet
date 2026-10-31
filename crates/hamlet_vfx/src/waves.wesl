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

// `SHORE_REACH` in `water.rs`.
const FIELD_REACH: f32 = 16.0;
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
// Height, frequency, speed and steepness of each.
const SHAPES = array<vec4<f32>, 5>(
    vec4<f32>(0.4, 1.0, 1.3, 0.2),
    vec4<f32>(0.15, 1.3, 1.6, 0.12),
    vec4<f32>(0.12, 2.0, 2.0, 0.17),
    vec4<f32>(0.01, 3.0, 3.0, 0.1),
    vec4<f32>(0.025, 4.0, 4.0, 0.08),
);

// Turns the wind's frame, where the wind blows toward -z, to the world's,
// where it blows `downwind`. A wave runs against the way its phase grows.
fn to_wind(downwind: vec2<f32>) -> mat2x2<f32> {
    let u = -downwind;
    return mat2x2<f32>(vec2<f32>(u.y, -u.x), u);
}

// One wave running along `way`, `along` the distance along it, lifted by a
// drifting noise. It is `group` of its full
// height, and `wobble` turns along its phase, so that it rises and falls
// in patches and its crests bend.
fn gerstner(
    noise: texture_2d<f32>,
    noise_sampler: sampler,
    pos: vec2<f32>,
    along: f32,
    t: f32,
    way: vec2<f32>,
    amplitude: f32,
    frequency: f32,
    speed: f32,
    steepness: f32,
    group: f32,
    wobble: f32,
) -> vec3<f32> {
    let lift = textureSampleLevel(noise, noise_sampler, pos * 0.2 + t * way * 0.07, 0.0).r;
    let phase = t * speed + along * frequency + wobble;
    let push = way * cos(phase) * steepness * group;
    return vec3<f32>(push.x, sin(phase) * amplitude * group + lift * 0.05 - 0.025, push.y);
}

// How much of its full height the wave `i` running along `way` keeps at `p`
// at time `t`, and how far its phase is turned there: each its own slow
// patches of noise tens of metres across, drifting with it.
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

// The five waves, each turned to `turn` by as much as `bent`: where it is
// whole, each runs along `turn`, `along` the distance along it.
fn five(
    noise: texture_2d<f32>,
    noise_sampler: sampler,
    p: vec2<f32>,
    t: f32,
    turn: vec2<f32>,
    along: f32,
    bent: bool,
    downwind: vec2<f32>,
    close: f32,
) -> vec3<f32> {
    let turned_to = to_wind(downwind);
    let ways = WAYS;
    let shapes = SHAPES;
    var d = vec3<f32>(0.0);
    for (var i = 0u; i < 5u; i++) {
        let open = turned_to * ways[i];
        let way = select(open, turn, bent);
        let distance = select(dot(open, p), along, bent);
        let s = shapes[i];
        let v = varied(noise, noise_sampler, p, t, way, i);
        // Turned to the shore, they rise and steepen as the water
        // shallows, `close` to it, then break. Further out they draw the
        // water toward their crests less: running in from every side of
        // a rock, they would squeeze all the water about it together.
        let rise = select(1.0, 1.0 + 0.6 * close, bent);
        let steep = select(s.w, s.w * mix(0.5, 1.0, close), bent);
        d += gerstner(noise, noise_sampler, p, distance, t, way, s.x * rise, s.y, s.z, steep, v.x, v.y);
    }
    // A broad, slow lift as well, so the surface is never the same twice.
    let broad = textureSampleLevel(noise, noise_sampler, p * 0.04 + vec2<f32>(t * 0.01, -t * 0.013), 0.0).r;
    d.y += broad * 0.08 - 0.04;
    return d;
}

// How far the waves move the point `pos` metres across at time `t`, where
// the shore's field `shore` lies over `field` on the map and the wind blows
// `downwind`. The waves turn to the shore within `bend` metres of it, all
// the way by halfway in, and within `calm.x` metres of it die down, to
// `calm.y` of their height at the water's edge, as shallow water stills
// them.
fn wave(
    noise: texture_2d<f32>,
    noise_sampler: sampler,
    shore: texture_2d<f32>,
    shore_sampler: sampler,
    field: vec4<f32>,
    pos: vec2<f32>,
    t: f32,
    scale: f32,
    bend: f32,
    calm: vec2<f32>,
    downwind: vec2<f32>,
) -> vec3<f32> {
    let p = pos / scale;
    let open = five(noise, noise_sampler, p, t, vec2<f32>(0.0), 0.0, false, downwind, 0.0);
    let near = coast(shore, shore_sampler, field, pos, bend, calm);
    let calmed = near.gain * scale;
    if near.turned <= 0.0 {
        return open * calmed;
    }
    let shoreward = five(noise, noise_sampler, p, t, -near.landward, near.along / scale, true, downwind, near.close);
    return mix(open, shoreward, near.turned) * calmed;
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

// How the shore bears on the waves at the point `pos` metres across, where
// its field `shore` lies over `field` on the map. They turn to it within
// `bend` metres of it, all the way by halfway in, and within `calm.x` metres
// of it die down, to `calm.y` of their height at the water's edge.
fn coast(
    shore: texture_2d<f32>,
    shore_sampler: sampler,
    field: vec4<f32>,
    pos: vec2<f32>,
    bend: f32,
    calm: vec2<f32>,
) -> Coast {
    // The field of the distance to the shore here: in red, the distance
    // over `FIELD_REACH`, above zero over water; the way to the shore in
    // green and blue; how much of it is sea, and not a river or a pool, in
    // alpha. Beyond it lies open sea.
    let uv = (pos - field.xy) / field.zw;
    let outside = any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0));
    let at = select(textureSampleLevel(shore, shore_sampler, uv, 0.0), vec4<f32>(1.0, 0.0, 0.0, 1.0), outside);
    let d = at.r * FIELD_REACH;
    let gain = mix(calm.y, 1.0, smoothstep(0.0, max(calm.x, 1e-3), d));
    // Only the sea's waves turn to the land. A river is narrow, and its
    // waves would turn to both banks at once and meet down its middle.
    var turned = (1.0 - smoothstep(bend * 0.55, max(bend, 1e-3), d)) * at.a;
    var landward = vec2<f32>(0.0, 1.0);
    if dot(at.gb, at.gb) < 1e-6 {
        turned = 0.0;
    } else {
        landward = normalize(at.gb);
    }
    // Running in to the land, the phase falls toward the shore, so each
    // crest comes in from the sea.
    let close = 1.0 - clamp(d / max(bend, 1e-3), 0.0, 1.0);
    return Coast(gain, turned, close, d, landward);
}

// How much of its height and how far its phase is turned each of the five
// waves keeps at `p` at time `t`, as `varied` has it, each turned to `turn`
// by as much as `bent`.
fn groups(
    noise: texture_2d<f32>,
    noise_sampler: sampler,
    p: vec2<f32>,
    t: f32,
    turn: vec2<f32>,
    bent: bool,
    downwind: vec2<f32>,
) -> array<vec2<f32>, 5> {
    let turned_to = to_wind(downwind);
    let ways = WAYS;
    var out: array<vec2<f32>, 5>;
    for (var i = 0u; i < 5u; i++) {
        out[i] = varied(noise, noise_sampler, p, t, select(turned_to * ways[i], turn, bent), i);
    }
    return out;
}

// How the five waves, laid as `five` lays them, keeping `kept` of their
// height and phase, draw the surface together about `p` at time `t`: how
// much further they push it along x and along z as `p` moves along x and
// along z, as x by x, x by z, z by x and z by z, the drifting noise's push
// by `streak` too. Exact for the waves themselves; the noise's slope is
// `slope`, found a few of its texels wide, so the streaks it draws are
// crisp along the crests, not flecks.
fn pull(
    slope: texture_2d<f32>,
    slope_sampler: sampler,
    p: vec2<f32>,
    t: f32,
    streak: vec2<f32>,
    turn: vec2<f32>,
    along: f32,
    bent: bool,
    downwind: vec2<f32>,
    close: f32,
    kept: array<vec2<f32>, 5>,
) -> vec4<f32> {
    let turned_to = to_wind(downwind);
    let ways = WAYS;
    let shapes = SHAPES;
    var by = kept;
    var d = vec4<f32>(0.0);
    for (var i = 0u; i < 5u; i++) {
        let open = turned_to * ways[i];
        let way = select(open, turn, bent);
        let distance = select(dot(open, p), along, bent);
        let s = shapes[i];
        let v = by[i];
        let steep = select(s.w, s.w * mix(0.5, 1.0, close), bent);
        let pinch = steep * v.x * s.y * sin(t * s.z + distance * s.y + v.y);
        d -= pinch * vec4<f32>(way.x * way.x, way.x * way.y, way.y * way.x, way.y * way.y);
        let uv = p * 0.2 + t * way * 0.07;
        let g = textureSampleLevel(slope, slope_sampler, uv, 0.0).rg * SLOPE_MOST * 0.2;
        d += vec4<f32>(streak.x * g.x, streak.x * g.y, streak.y * g.x, streak.y * g.y);
    }
    return d;
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
