// The wind, as `WindSettings::strength` in `wind.rs` has it: one way
// across the whole world, veering slowly, and a strength that rolls
// downwind in bands, smaller gusts within each. The two must give the same
// answer, so a change to one is a change to the other.

import hamlet_vfx::noise::fbm;

struct Wind {
    // x: the way it blows on the whole, in radians from north. y: how far
    // it veers to either side. w: the share of its strength it keeps
    // between gusts.
    flow: vec4<f32>,
    // x: one over the breadth of a band of strong wind. y: how far the
    // bands roll in a second, in breadths. zw: the way they roll, as world
    // x and z.
    scale: vec4<f32>,
}

// The seed of the wind's noise. `SEED` in `wind.rs`.
const SEED: u32 = 7u;

// The way the wind blows on the ground at `t` seconds, as world x and z,
// as `WindSettings::blowing` in `wind.rs` has it.
fn blowing(wind: Wind, t: f32) -> vec2<f32> {
    let a = wind.flow.x + wind.flow.y * sin(t * 0.11);
    return vec2<f32>(sin(a), cos(a));
}

// How hard the wind blows at a place on the ground at `t` seconds, from
// the calm share to 1.
fn strength(wind: Wind, at: vec2<f32>, t: f32) -> f32 {
    let rolled = wind.scale.zw * (t * wind.scale.y);
    let band = 0.5 + 0.5 * fbm(at * wind.scale.x - rolled, 2u, SEED);
    let gust = smoothstep(0.3, 0.75, band);
    return wind.flow.w + (1.0 - wind.flow.w) * gust;
}
