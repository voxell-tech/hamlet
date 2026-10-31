// The realm's seeded gradient noise, as `noise.rs` in `hamlet_realm` has
// it, so that the ground's colour comes out as the land's other noise
// does. The hash is the same 32-bit steps; the gradients are worked out
// rather than looked up, which differs from the table by rounding alone.

// PCG's output step: `mix` in `noise.rs`.
fn scramble(x: u32) -> u32 {
    let state = x * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}

// `key(&[seed, x, z])` in `noise.rs`.
fn key(seed: u32, x: i32, z: i32) -> u32 {
    var h = scramble(0x51ed270bu ^ seed);
    h = scramble(h ^ bitcast<u32>(x));
    return scramble(h ^ bitcast<u32>(z));
}

// One of sixteen directions a sixteenth of a turn apart.
fn gradient(x: i32, z: i32, seed: u32) -> vec2<f32> {
    let a = f32(key(seed, x, z) & 15u) * 0.39269908;
    return vec2<f32>(cos(a), sin(a));
}

// Gradient noise in about -1..1, with a quintic fade.
fn perlin(p: vec2<f32>, seed: u32) -> f32 {
    let cell = floor(p);
    let f = p - cell;
    let x = i32(cell.x);
    let z = i32(cell.y);
    let fade = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let a = dot(gradient(x, z, seed), f);
    let b = dot(gradient(x + 1, z, seed), f - vec2<f32>(1.0, 0.0));
    let c = dot(gradient(x, z + 1, seed), f - vec2<f32>(0.0, 1.0));
    let d = dot(gradient(x + 1, z + 1, seed), f - vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, fade.x), mix(c, d, fade.x), fade.y) * 1.41;
}

// A fractal sum of `octaves` layers of `perlin`, in about -1..1.
fn fbm(p: vec2<f32>, octaves: u32, seed: u32) -> f32 {
    var sum = 0.0;
    var amp = 1.0;
    var freq = 1.0;
    var total = 0.0;
    for (var o = 0u; o < octaves; o++) {
        sum += amp * perlin(p * freq, seed + o);
        total += amp;
        amp *= 0.5;
        freq *= 2.03;
    }
    return sum / total;
}
