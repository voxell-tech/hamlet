// What every pass of the lily pads shares: how a pad rides the water,
// moved as one by what the water's waves do to the surface at its middle,
// and its strokes, which it is cut to.

import bevy_pbr::render::mesh_view_bindings::view;
import hamlet_vfx::shore::coast_at;
import hamlet_vfx::waves::wave;

// Mirrors `Water` in `water.wesl`; the fields are explained on
// `WaterUniform` in `water.rs`.
struct Water {
    shallow: vec4<f32>,
    mid: vec4<f32>,
    deep: vec4<f32>,
    foam_color: vec4<f32>,
    foam: vec4<f32>,
    time: vec4<f32>,
    calm: vec4<f32>,
    wind: vec4<f32>,
    sky: vec4<f32>,
    fading: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> water: Water;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var noise: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var noise_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(110) var strokes: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(111) var strokes_sampler: sampler;

// How far from the eye, in units of the waves, the water lies flat.
// `FLAT` in `water.wesl`.
const FLAT: f32 = 55.0;

// Where the point `local` of a pad moved by `world_from_local`, which lays
// it on the still water, rides: as far as the waves move the water's
// surface at the pad's middle, and not at all where the water lies flat.
fn ridden(local: vec3<f32>, world_from_local: mat4x4<f32>) -> vec3<f32> {
    let middle = world_from_local[3].xyz;
    let at = (world_from_local * vec4<f32>(local, 1.0)).xyz;
    let scale = water.time.y;
    let kept = 1.0 - smoothstep(25.0 * scale, FLAT * scale, distance(middle, view.world_position));
    if kept <= 0.0 {
        return at;
    }
    let near = coast_at(middle.xz, water.time.z, water.calm.xy);
    return at + wave(noise, noise_sampler, near, middle.xz, water.time.x, scale, water.wind.xy) * kept;
}
