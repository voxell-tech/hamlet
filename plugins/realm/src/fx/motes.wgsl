// Motes of light (motes.rs): each copy of a small ball is one mote,
// placed, sized and faded from its tag, the time and the params, so
// nothing is sent each frame but the age of a cloud that plays once.
// Every copy stands at the cloud's middle, as large as the cloud, so
// that it is drawn wherever one of its motes is.
// Params: 0 the colour, already as bright as it shines, and the way
// the motes move (w); 1 the size of each copy (x), the number of
// motes (y), the seconds since the cloud set off (z) and a number of
// its own (w). The rest is the way's own:
// a swarm (0): 2 the reach of the ground they rise from, their
//   highest, their speed and their size; 3 the seconds each lives and
//   how many are born each second.
// a burst (1): 2 the reach of the ground they leap from, their speed,
//   the seconds each lives at most and their size.
// a rise (2): 2 the reach of the ground they rise from, the seconds
//   over which they set out, the seconds each lasts and how far they
//   climb; 3 how far they stray and their size.
// a flight (3): 2 where they set out from and how far apart; 3 where
//   they land and the seconds on the way; 4 how high they arc, how far
//   they spread and their size.

const SWARM: f32 = 0.0;
const BURST: f32 = 1.0;
const RISE: f32 = 2.0;

const TAU: f32 = 6.2831853;

struct Mote {
    // From the cloud's middle.
    at: vec3<f32>,
    // The radius of its ball; none while it is not there.
    size: f32,
    alpha: f32,
}

// A number from 0 to 1 that depends only on `n`.
fn chance(n: f32) -> f32 {
    return abs(fract(sin(n * 12.9898 + 4.1414) * 43758.547));
}

// A number from 0 to 1 for the mote `i` in its life `turn`, and
// `salt`.
fn roll(i: u32, turn: u32, salt: u32) -> f32 {
    var h = i * 747796405u + turn * 2891336453u + salt * 277803737u + 1u;
    h = ((h >> ((h >> 28u) + 4u)) ^ h) * 277803737u;
    h = (h >> 22u) ^ h;
    return f32(h) / 4294967295.0;
}

// A point of a disc `reach` wide, all of it as likely.
fn disc(reach: f32, a: f32, b: f32) -> vec3<f32> {
    let r = reach * sqrt(a);
    let t = b * TAU;
    return vec3<f32>(cos(t) * r, 0.0, sin(t) * r);
}

// Between the keys of `x`, each a place and a value.
fn keyed(x: f32, k0: vec2<f32>, k1: vec2<f32>, k2: vec2<f32>, k3: vec2<f32>, k4: vec2<f32>) -> f32 {
    if x < k1.x {
        return mix(k0.y, k1.y, (x - k0.x) / (k1.x - k0.x));
    }
    if x < k2.x {
        return mix(k1.y, k2.y, (x - k1.x) / (k2.x - k1.x));
    }
    if x < k3.x {
        return mix(k2.y, k3.y, (x - k2.x) / (k3.x - k2.x));
    }
    return mix(k3.y, k4.y, clamp((x - k3.x) / (k4.x - k3.x), 0.0, 1.0));
}

// Fireflies: each is born in turn, lives a while, blinking on, dimming
// and glowing again before it goes, and is born again elsewhere.
fn swarm(i: u32, time: f32) -> Mote {
    let count = params[1].y;
    let life = params[3].x;
    let every = count / max(params[3].y, 1e-3);
    let phase = f32(i) / count;
    let turn = max(floor(time / every - phase), 0.0);
    let age = time - (turn + phase) * every;
    if age < 0.0 {
        return Mote(vec3<f32>(0.0), 0.0, 0.0);
    }
    let r = u32(turn);
    let lives = life * (0.6 + 0.4 * roll(i, r, 0u));
    if age > lives {
        return Mote(vec3<f32>(0.0), 0.0, 0.0);
    }
    let reach = params[2].x;
    let high = params[2].y;
    let speed = params[2].z;
    let start = disc(reach, roll(i, r, 1u), roll(i, r, 2u))
        + vec3<f32>(0.0, mix(0.4, high, roll(i, r, 3u)), 0.0);
    let wander = (vec3<f32>(roll(i, r, 4u), roll(i, r, 5u), roll(i, r, 6u)) - 0.5)
        * vec3<f32>(2.0, 0.6, 2.0) * speed;
    let x = age / lives;
    let alpha = keyed(x, vec2<f32>(0.0, 0.0), vec2<f32>(0.12, 1.0), vec2<f32>(0.45, 0.35), vec2<f32>(0.7, 1.0), vec2<f32>(1.0, 0.0));
    return Mote(start + wander * age, params[2].w * 0.5, alpha);
}

// Embers that leap from the ground at once, slowed by the air and
// lifted by the heat, shrinking as they go.
fn burst(i: u32, age: f32) -> Mote {
    let salt = u32(params[1].w);
    let reach = params[2].x;
    let speed = params[2].y;
    let life = params[2].z;
    let lives = life * mix(0.45, 1.0, roll(i, salt, 0u));
    if age > lives {
        return Mote(vec3<f32>(0.0), 0.0, 0.0);
    }
    let start = disc(reach, roll(i, salt, 1u), roll(i, salt, 2u));
    let side = (vec2<f32>(roll(i, salt, 3u), roll(i, salt, 4u)) - 0.5) * 1.6;
    let up = speed * mix(0.5, 1.0, roll(i, salt, 5u));
    let set_off = vec3<f32>(side.x, up, side.y);
    // Dragged at 1.1 a second and lifted at 0.9.
    let drag = 1.1;
    let lift = vec3<f32>(0.0, 0.9 / drag, 0.0);
    let at = start + lift * age + (set_off - lift) * (1.0 - exp(-drag * age)) / drag;
    let x = age / lives;
    let alpha = keyed(x, vec2<f32>(0.0, 1.0), vec2<f32>(0.3, 0.9), vec2<f32>(0.6, 0.8), vec2<f32>(0.8, 0.4), vec2<f32>(1.0, 0.0));
    return Mote(at, params[2].w * 0.5 * (1.0 - x), alpha);
}

// Embers that set out one after another and climb, swaying, flaring
// as they leave the ground and dying on the way up.
fn rise(i: u32, age: f32) -> Mote {
    let own = f32(i) + chance(params[1].w + f32(i));
    let around = chance(own) * TAU;
    let away = sqrt(chance(own + 0.5)) * params[2].x;
    let start = vec3<f32>(cos(around), 0.0, sin(around)) * away;
    let wait = chance(own + 0.25) * params[2].y;
    let u = (age - wait) / params[2].z;
    if u < 0.0 || u >= 1.0 {
        return Mote(vec3<f32>(0.0), 0.0, 1.0);
    }
    let sway = vec3<f32>(sin(u * 5.0 + own), 0.0, cos(u * 4.0 + own * 1.7)) * params[3].x;
    let up = params[2].w * (0.6 + 0.4 * chance(own + 0.75)) * (1.0 - (1.0 - u) * (1.0 - u));
    let size = min(u * 6.0, 1.0) * (1.0 - u);
    return Mote(start + vec3<f32>(0.0, up, 0.0) + sway * u, params[3].y * size, 1.0);
}

// Motes that fly from one place to another along an arc, each setting
// out a little after the last and all arriving together.
fn flight(i: u32, age: f32) -> Mote {
    let own = f32(i) + chance(params[1].w + f32(i));
    let time = params[3].w;
    let away = vec3<f32>(chance(own) - 0.5, chance(own + 0.3), chance(own + 0.6) - 0.5) * params[2].w;
    let wait = chance(own + 0.25) * time * 0.35;
    let u = (age - wait) / (time - wait);
    if u < 0.0 || u >= 1.0 {
        return Mote(vec3<f32>(0.0), 0.0, 1.0);
    }
    let along = u * u * (3.0 - 2.0 * u);
    let arc = sin(u * 3.14159265);
    let side = vec3<f32>(sin(own * 3.1), 0.0, cos(own * 2.3)) * params[4].y;
    let at = mix(params[2].xyz + away, params[3].xyz, along) + vec3<f32>(0.0, params[4].x * arc, 0.0) + side * arc;
    return Mote(at, params[4].z * sqrt(arc), 1.0);
}

fn mote(i: u32, time: f32) -> Mote {
    let way = params[0].w;
    let age = params[1].z;
    if way < SWARM + 0.5 {
        return swarm(i, time);
    }
    if way < BURST + 0.5 {
        return burst(i, age);
    }
    if way < RISE + 0.5 {
        return rise(i, age);
    }
    return flight(i, age);
}

fn displace(vertex: Vertex) -> vec3<f32> {
    let m = mote(vertex.tag, vertex.time);
    let size = params[1].x;
    return (m.at + vertex.position * m.size) / size - vertex.position;
}

fn shade(surface: Surface) -> vec4<f32> {
    let m = mote(surface.tag, surface.time);
    return vec4<f32>(params[0].rgb, m.alpha);
}
