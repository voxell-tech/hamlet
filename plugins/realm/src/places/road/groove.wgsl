// The groove: a slate bed with a strip of pewter down its middle, and,
// drawn over it in a material of its own, the light of the strip. UV.x
// is the distance along the road and UV.y runs from -1 to 1 across it.
// Params: 0 the bed's colour, 1 the strip's, 2 the light's colour and
// its strength, 3 the lit stretch (x to y), 4 the way it is lit (x),
// 5 the strip's half-width (x) and 1 for the light, 0 for the bed (y).

// The length of one slab of the bed.
const SLAB: f32 = 0.42;

fn hash(x: f32) -> f32 {
    return fract(sin(x * 127.1 + 311.7) * 43758.5453);
}

// How lit the strip is at distance `s` along it, at time `t`: breathing
// while its agent waits, broken and stuttering where one has faulted.
fn glow(s: f32, t: f32) -> f32 {
    let a = params[3].x;
    let b = params[3].y;
    let within = smoothstep(a - 0.12, a + 0.12, s) * (1.0 - smoothstep(b - 0.12, b + 0.12, s));
    if params[4].x < 3.5 {
        return within * (0.8 + 0.2 * sin(t * 3.14159265));
    }
    let cell = floor(s / 0.55);
    let gap = step(hash(cell * 3.1 + 0.5), 0.2);
    let tick = floor(t * 2.6 + hash(cell) * 3.0);
    let drop = step(hash(cell * 13.7 + tick * 7.3), 0.4);
    let ends = smoothstep(0.0, 0.06, min(fract(s / 0.55), 1.0 - fract(s / 0.55)) * 0.55);
    return within * (1.0 - gap) * mix(1.0, 0.06, drop) * mix(0.2, 1.0, ends);
}

fn shade(surface: Surface) -> vec4<f32> {
    let s = surface.uv.x;
    let across = abs(surface.uv.y);
    // The strip is held at a pixel or more so it never shimmers away.
    let px = fwidth(surface.uv.y);
    let half_width = max(params[5].x, px);
    let strip = 1.0 - smoothstep(half_width - px * 0.75, half_width + px * 0.75, across);
    if params[5].y > 0.5 {
        let lit = glow(s, surface.time);
        return vec4<f32>(params[2].rgb * params[2].a * lit * strip, 1.0);
    }
    let along = s / SLAB;
    let joint = smoothstep(0.0, 0.035, min(fract(along), 1.0 - fract(along)) * SLAB);
    let bed = params[0].rgb * (0.82 + 0.36 * hash(floor(along))) * mix(0.55, 1.0, joint);
    return vec4<f32>(mix(bed, params[1].rgb, strip) * surface.color.rgb, 1.0);
}
