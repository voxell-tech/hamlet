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
    // x: how far the whole wind's swelling turns in a second, in radians.
    // y and z: how hard the whole wind blows at the depth of a lull and
    // at the height of a swell, `WindChoice` in `wind.rs`.
    swell: vec4<f32>,
}

// The seed of the wind's noise. `SEED` in `wind.rs`.
const SEED: u32 = 7u;

// The way the wind blows on the ground at `t` seconds, as world x and z,
// as `WindSettings::blowing` in `wind.rs` has it.
fn blowing(wind: Wind, t: f32) -> vec2<f32> {
    let a = wind.flow.x + wind.flow.y * sin(t * 0.11);
    return vec2<f32>(sin(a), cos(a));
}

// How hard the whole wind blows at `t` seconds, from the least it is let
// blow at the depth of a lull to the most at the height of a swell: it
// rises and falls everywhere at once.
fn overall(wind: Wind, t: f32) -> f32 {
    let a = t * wind.swell.x;
    let rise = 0.5 + 0.25 * (sin(a) + sin(a * 1.618 + 1.3));
    return mix(wind.swell.y, wind.swell.z, smoothstep(0.0, 1.0, rise));
}

// How hard the wind blows at a place on the ground at `t` seconds: the
// calm share between gusts, all of it within one, times how hard the
// whole wind blows.
fn strength(wind: Wind, at: vec2<f32>, t: f32) -> f32 {
    let rolled = wind.scale.zw * (t * wind.scale.y);
    let band = 0.5 + 0.5 * fbm(at * wind.scale.x - rolled, 2u, SEED);
    let gust = smoothstep(0.3, 0.75, band);
    return (wind.flow.w + (1.0 - wind.flow.w) * gust) * overall(wind, t);
}
