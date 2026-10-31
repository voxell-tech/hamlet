// The foam on the water, kept from frame to frame in a map laid over the
// world about the eye. Ported from an MIT-licensed water shader; its notice
// is in `water/LICENSE`.
//
// Foam is born where the waves squeeze the surface together, measured by how
// much area a texel's patch of it keeps, and only where a noise lets it. The
// last frame's foam stays, moved with the map, fading and eaten away by
// another noise, and the more of the two is kept.

import hamlet_vfx::waves::wave;

// The fields are explained on `FoamUniform` in `foam.rs`.
struct Foam {
    middle: vec4<f32>,
    shift: vec4<f32>,
    fade: vec4<f32>,
    field: vec4<f32>,
    bend: vec4<f32>,
    wind: vec4<f32>,
}

@group(0) @binding(0) var<uniform> foam: Foam;
@group(0) @binding(1) var noise: texture_2d<f32>;
@group(0) @binding(2) var noise_sampler: sampler;
@group(0) @binding(3) var last: texture_2d<f32>;
@group(0) @binding(4) var last_sampler: sampler;
@group(0) @binding(5) var next: texture_storage_2d<rgba16float, write>;
@group(0) @binding(6) var shore: texture_2d<f32>;
@group(0) @binding(7) var shore_sampler: sampler;

// How far the waves move the point `pos` metres across, drawn into streaks.
fn moved(pos: vec2<f32>) -> vec3<f32> {
    return wave(
        noise,
        noise_sampler,
        shore,
        shore_sampler,
        foam.field,
        pos,
        foam.shift.z,
        foam.shift.w,
        foam.bend.x,
        foam.bend.yz,
        foam.wind.xy,
        foam.fade.zw,
    );
}

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let side = textureDimensions(next);
    if any(id.xy >= side) {
        return;
    }
    let uv = (vec2<f32>(id.xy) + 0.5) / vec2<f32>(side);
    let coverage = foam.middle.z;
    let dt = foam.middle.w;

    // The patch of surface one texel wide here, as the waves move it.
    let step = coverage / vec2<f32>(side);
    let at = foam.middle.xy + (uv - 0.5) * coverage;
    let x = at + vec2<f32>(step.x, 0.0);
    let z = at + vec2<f32>(0.0, step.y);
    let here = at + moved(at).xz;
    let along_x = x + moved(x).xz - here;
    let along_z = z + moved(z).xz - here;
    let kept = (along_x.x * along_z.y - along_x.y * along_z.x) / (step.x * step.y);
    let squeezed = 1.0 - smoothstep(0.0, 0.7, kept);

    let world_uv = at / 15.0;
    let allowed = smoothstep(0.2, 0.7, textureSampleLevel(noise, noise_sampler, world_uv, 0.0).r) * 8.0;
    // Held between 0 and 1, as a frame's foam is drawn.
    let fresh = clamp(squeezed * allowed, 0.0, 1.0);

    // The last frame's foam, where this texel was then.
    let was = uv + foam.shift.xy;
    var left = vec2<f32>(0.0);
    if all(was >= vec2<f32>(0.0)) && all(was <= vec2<f32>(1.0)) {
        left = textureSampleLevel(last, last_sampler, was, 0.0).rg;
    }
    let eaten = textureSampleLevel(noise, noise_sampler, world_uv * 24.0, 0.0).r * dt * foam.fade.y;
    left -= vec2<f32>(eaten, eaten * 0.1);
    left *= exp(-foam.fade.x * dt * vec2<f32>(1.0, 0.3));
    left = max(left, vec2<f32>(0.0));

    textureStore(next, vec2<i32>(id.xy), vec4<f32>(max(fresh, left.x), left.y, 0.0, 1.0));
}
