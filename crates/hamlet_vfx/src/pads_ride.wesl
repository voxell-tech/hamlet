// How a lily pad rides the water, moved and leaned as one by what the
// water's waves do to the surface under it, and the strokes it is cut to.

import bevy_pbr::render::mesh_view_bindings::view;
import hamlet_vfx::clock::scene_time;
import hamlet_vfx::shore::coast_at;
import hamlet_vfx::waves::{Coast, wave};

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

// How much of the slope of the water under it a pad leans with, and the
// most it leans, as a slope, so it only ever rocks a little.
const LEAN: f32 = 1.0;
const LEAN_MOST: f32 = 0.2;

// How a pad rides the water: how far the waves move its middle, and the way
// its face turns to.
struct Ride {
    lift: vec3<f32>,
    up: vec3<f32>,
}

// Where the waves, `kept` of their height, move the water's surface over
// the still point `p`, with the shore as it bears at `near`, which must be
// found at `p` itself: near it the waves' phase is the distance to it.
fn surface(p: vec2<f32>, near: Coast, kept: f32) -> vec3<f32> {
    let lift = wave(noise, noise_sampler, near, p, scene_time(water.time.x), water.time.y, water.wind.xy);
    return vec3<f32>(p.x, 0.0, p.y) + lift * kept;
}

// How the pad laid on the still water by `world_from_local` rides: moved as
// the waves move the surface at its middle, and leaned with the surface's
// slope along the waves' way, taken half the pad's width on. Still where
// the water lies flat.
fn ride(world_from_local: mat4x4<f32>) -> Ride {
    let middle = world_from_local[3].xyz;
    let scale = water.time.y;
    let kept = 1.0 - smoothstep(25.0 * scale, FLAT * scale, distance(middle, view.world_position));
    let still = Ride(vec3<f32>(0.0), vec3<f32>(0.0, 1.0, 0.0));
    if kept <= 0.0 {
        return still;
    }
    let c = middle.xz;
    let near = coast_at(c, water.time.z, water.calm.xy);
    // Near the shore the waves turn to run in to it.
    let runs = mix(water.wind.xy, near.landward, near.turned);
    let way = select(vec2<f32>(0.0, 1.0), normalize(runs), dot(runs, runs) > 1e-6);
    let r = max(0.5 * length(world_from_local[0].xyz), 0.2);
    let here = surface(c, near, kept);
    let ahead = c + way * r;
    let on = surface(ahead, coast_at(ahead, water.time.z, water.calm.xy), kept);
    let rise = (on.y - here.y) / max(dot(on.xz - here.xz, way), 1e-3);
    let lean = clamp(rise * LEAN, -LEAN_MOST, LEAN_MOST);
    return Ride(
        here - vec3<f32>(c.x, 0.0, c.y),
        normalize(vec3<f32>(-way.x * lean, 1.0, -way.y * lean)),
    );
}

// `v` turned as straight up is turned to `up`, which leans less than a
// right angle.
fn turned(v: vec3<f32>, up: vec3<f32>) -> vec3<f32> {
    let w = cross(vec3<f32>(0.0, 1.0, 0.0), up);
    return v * up.y + cross(w, v) + w * dot(w, v) / (1.0 + up.y);
}

// Where the point `local` of the pad laid by `world_from_local` rides, as
// `ride` has the pad.
fn ridden(local: vec3<f32>, world_from_local: mat4x4<f32>, ride: Ride) -> vec3<f32> {
    let middle = world_from_local[3].xyz;
    let at = (world_from_local * vec4<f32>(local, 1.0)).xyz;
    return middle + ride.lift + turned(at - middle, ride.up);
}
