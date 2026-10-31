// The wind, as `WindSettings::strength` in `wind.rs` has it: one way
// across the whole world, and a strength that rolls downwind in bands.
// The two must give the same answer, so a change to one is a change to
// the other.

struct Wind {
    // xy: the way it blows on the ground, as world x and z. z: time in
    // seconds. w: the share of its strength it keeps between gusts.
    flow: vec4<f32>,
    // x: one over the breadth of a band of strong wind. y: the band's
    // speed. z and w: the same for the shiver within it.
    scale: vec4<f32>,
}

fn hash(x: i32, y: i32) -> f32 {
    var h = (bitcast<u32>(x) * 0x8da6b343u) ^ (bitcast<u32>(y) * 0xd8163841u);
    h ^= h >> 13u;
    h *= 0x5bd1e995u;
    h ^= h >> 15u;
    return f32(h & 0xffffffu) / 16777216.0;
}

fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = p - i;
    let u = f * f * (3.0 - 2.0 * f);
    let x = i32(i.x);
    let y = i32(i.y);
    return mix(
        mix(hash(x, y), hash(x + 1, y), u.x),
        mix(hash(x, y + 1), hash(x + 1, y + 1), u.x),
        u.y,
    );
}

// How hard the wind blows at a place on the ground, from the calm share
// to 1.
fn strength(wind: Wind, at: vec2<f32>) -> f32 {
    let along = wind.flow.xy * wind.flow.z;
    let band = noise((at - along * wind.scale.y) * wind.scale.x);
    let shiver = noise((at - along * wind.scale.w) * wind.scale.z);
    let gust = smoothstep(0.35, 0.85, band) * (0.7 + 0.3 * shiver);
    return wind.flow.w + (1.0 - wind.flow.w) * gust;
}
