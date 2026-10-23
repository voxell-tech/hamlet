// The inlay: a slate bed with a strip of pewter down its middle and,
// drawn over it in a material of its own, the light of the strip. UV.x
// is the distance along the line and UV.y runs from -1 to 1 across it.
// Params: 0 the bed's colour, 1 the strip's, 2 the light's colour and
// its strength, 3 the lit stretch (x to y) and the line's length (w),
// 4 the way it is lit (x), where a mote or a filling front is (z) and
// the seconds the trace behind a mote takes to fade to half (w), 5 the
// strip's half-width (x), 1 for the light and 0 for the bed (y), the
// speed of a mote (z) and where it set out from (w).

// The ways the strip is lit.
const TRAVEL: f32 = 1.0;
const FILL: f32 = 2.0;
const BREATH: f32 = 3.0;
const STUTTER: f32 = 4.0;

// The length of one slab of the bed.
const SLAB: f32 = 0.42;

// An ember's light against a lit strip's: 1.5 against 4.
const EMBER: f32 = 0.375;

fn hash(x: f32) -> f32 {
    return fract(sin(x * 127.1 + 311.7) * 43758.5453);
}

// How lit the strip is at distance `s` along it, at time `t`, 0 to
// about 1.5: a mote that runs along it and the trace it leaves, a
// front that fills it, a breath while its agent waits, and a broken
// stutter where one has faulted.
fn glow(s: f32, t: f32) -> f32 {
    let mode = params[4].x;
    let a = params[3].x;
    let b = params[3].y;
    let within = smoothstep(a - 0.12, a + 0.12, s) * (1.0 - smoothstep(b - 0.12, b + 0.12, s));
    if mode < TRAVEL - 0.5 {
        return 0.0;
    }
    if mode < FILL - 0.5 {
        let mote = params[4].z;
        let behind = mote - s;
        let arrived = smoothstep(params[3].w, params[3].w + 0.4, mote);
        let head = exp(-pow(behind / 0.2, 2.0)) * 1.5 * (1.0 - arrived);
        if behind < 0.0 || s < params[5].w {
            return head;
        }
        let tail = exp(-behind / 0.8) * 0.7 * (1.0 - arrived);
        let age = behind / max(params[5].z, 0.01);
        let trace = EMBER * pow(0.5, age / max(params[4].w, 0.01)) * (1.0 - smoothstep(100.0, 120.0, age));
        return max(head, tail + trace);
    }
    if mode < BREATH - 0.5 {
        let front = params[4].z;
        let body = smoothstep(a - 0.1, a + 0.1, s) * (1.0 - smoothstep(front - 0.3, front, s));
        let lead = exp(-pow((s - front + 0.15) / 0.22, 2.0)) * 0.5;
        return body * 0.85 + lead;
    }
    if mode < STUTTER - 0.5 {
        // 0.5 Hz, between 60% and 100%.
        return within * (0.8 + 0.2 * sin(t * 3.14159265));
    }
    let cell = floor(s / 0.55);
    let gap = step(hash(cell * 3.1 + 0.5), 0.2);
    let tick = floor(t * 2.6 + hash(cell) * 3.0);
    let drop = step(hash(cell * 13.7 + tick * 7.3), 0.4);
    let ends = smoothstep(0.0, 0.06, min(fract(s / 0.55), 1.0 - fract(s / 0.55)) * 0.55);
    return within * (1.0 - gap) * mix(1.0, 0.06, drop) * mix(0.2, 1.0, ends);
}

fn shade(surface: Surface) -> Shading {
    let s = surface.uv.x;
    let across = abs(surface.uv.y);
    // The strip is held at a pixel or more so it never shimmers away.
    let px = fwidth(surface.uv.y);
    let half_width = max(params[5].x, px);
    let strip = 1.0 - smoothstep(half_width - px * 0.75, half_width + px * 0.75, across);
    if params[5].y > 0.5 {
        let lit = glow(s, surface.time);
        return Shading(vec4<f32>(params[2].rgb * params[2].a * lit * strip, 1.0), vec3<f32>(0.0), 0.0, 1.0, 0.0);
    }
    let along = s / SLAB;
    let joint = smoothstep(0.0, 0.035, min(fract(along), 1.0 - fract(along)) * SLAB);
    let bed = params[0].rgb * (0.82 + 0.36 * hash(floor(along))) * mix(0.55, 1.0, joint);
    // The pewter strip gleams and the slate is dull.
    let c = vec4<f32>(mix(bed, params[1].rgb, strip) * surface.color.rgb, 1.0);
    return Shading(c, vec3<f32>(0.0), strip * 0.5, mix(0.85, 0.45, strip), 0.5);
}
