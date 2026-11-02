// The wind, as `WindSettings::strength` in `wind.rs` has it: one way
// across the whole world, and a strength that rolls downwind in bands,
// smaller gusts within each. The two must give the same answer, so a change
// to one is a change to the other.

import hamlet_vfx::noise::fbm;

struct Wind {
    // xy: the way it blows on the ground, as world x and z. z: time in
    // seconds. w: the share of its strength it keeps between gusts.
    flow: vec4<f32>,
    // x: one over the breadth of a band of strong wind. y: the band's
    // speed.
    scale: vec4<f32>,
}

// The seed of the wind's noise. `SEED` in `wind.rs`.
const SEED: u32 = 7u;

// How hard the wind blows at a place on the ground, from the calm share
// to 1.
fn strength(wind: Wind, at: vec2<f32>) -> f32 {
    let along = wind.flow.xy * wind.flow.z;
    let band = 0.5 + 0.5 * fbm((at - along * wind.scale.y) * wind.scale.x, 2u, SEED);
    let gust = smoothstep(0.3, 0.75, band);
    return wind.flow.w + (1.0 - wind.flow.w) * gust;
}
