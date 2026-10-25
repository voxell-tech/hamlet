// The inlay: a slate bed with a strip of pewter down its middle that lights
// when something flows. UV.x is distance along the line in world units and
// UV.y runs from -1 to 1 across the bed. The same shader draws the soft wash
// beside the groove at night, on a wider ribbon with additive blending.

import bevy_pbr::render::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{VertexOutput, FragmentOutput},
};

struct Inlay {
    // rgb: the light's colour (linear). a: emissive strength when fully lit.
    hue: vec4<f32>,
    // x, y: the lit stretch. z: unused. w: total length.
    range: vec4<f32>,
    // x: mode. y: time in seconds. z: position of the mote or fill front.
    // w: half-life of the trace in seconds.
    anim: vec4<f32>,
    // x: half-width of the strip in UV.y. y: wash strength (0 on the groove
    // itself). z: speed of the mote. w: where the mote set out from.
    shape: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> inlay: Inlay;

// The colours of the unlit groove, rgb (linear).
struct Stone {
    bed: vec4<f32>,
    strip: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(101) var<uniform> stone: Stone;

const MODE_UNLIT: f32 = 0.0;
const MODE_TRAVEL: f32 = 1.0;
const MODE_FILL: f32 = 2.0;
const MODE_BREATH: f32 = 3.0;
const MODE_STUTTER: f32 = 4.0;

const SLAB: f32 = 0.42;
// An ember is 1.5 against a lit strength of 4.
const EMBER: f32 = 0.375;

fn hash(x: f32) -> f32 {
    return fract(sin(x * 127.1 + 311.7) * 43758.5453);
}

// How lit the line is at distance `s` along it, 0 to about 1.5.
fn glow(s: f32) -> f32 {
    let mode = inlay.anim.x;
    let t = inlay.anim.y;
    let a = inlay.range.x;
    let b = inlay.range.y;
    let within = smoothstep(a - 0.12, a + 0.12, s) * (1.0 - smoothstep(b - 0.12, b + 0.12, s));

    if mode < MODE_TRAVEL - 0.5 {
        return 0.0;
    }
    if mode < MODE_FILL - 0.5 {
        let mote = inlay.anim.z;
        let behind = mote - s;
        let arrived = smoothstep(inlay.range.w, inlay.range.w + 0.4, mote);
        let head = exp(-pow(behind / 0.2, 2.0)) * 1.5 * (1.0 - arrived);
        if behind < 0.0 || s < inlay.shape.w {
            return head;
        }
        let tail = exp(-behind / 0.8) * 0.7 * (1.0 - arrived);
        let age = behind / max(inlay.shape.z, 0.01);
        let trace = EMBER * pow(0.5, age / max(inlay.anim.w, 0.01)) * (1.0 - smoothstep(100.0, 120.0, age));
        return max(head, tail + trace);
    }
    if mode < MODE_BREATH - 0.5 {
        let front = inlay.anim.z;
        let body = smoothstep(a - 0.1, a + 0.1, s) * (1.0 - smoothstep(front - 0.3, front, s));
        let lead = exp(-pow((s - front + 0.15) / 0.22, 2.0)) * 0.5;
        return body * 0.85 + lead;
    }
    if mode < MODE_STUTTER - 0.5 {
        // 0.5 Hz, between 60% and 100%.
        return within * (0.8 + 0.2 * sin(t * 3.14159265));
    }
    // Fault: a line with pieces missing, and pieces that drop out unevenly.
    let cell = floor(s / 0.55);
    let gap = step(hash(cell * 3.1 + 0.5), 0.2);
    let tick = floor(t * 2.6 + hash(cell) * 3.0);
    let drop = step(hash(cell * 13.7 + tick * 7.3), 0.4);
    let ends = smoothstep(0.0, 0.06, min(fract(s / 0.55), 1.0 - fract(s / 0.55)) * 0.55);
    return within * (1.0 - gap) * mix(1.0, 0.06, drop) * mix(0.2, 1.0, ends);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    var out: FragmentOutput;
    let s = in.uv.x;
    let across = abs(in.uv.y);
    let lit = glow(s);
    let px = fwidth(in.uv.y);

    if inlay.shape.y > 0.0 {
        let fall = pow(max(1.0 - across, 0.0), 2.2);
        let wash = vec4<f32>(inlay.hue.rgb, fall * min(lit, 1.0) * inlay.shape.y);
        out.color = main_pass_post_lighting_processing(pbr_input, wash);
        return out;
    }

    // Hold the strip at two pixels or more so it never shimmers away.
    let half_width = max(inlay.shape.x, px);
    let strip = 1.0 - smoothstep(half_width - px * 0.75, half_width + px * 0.75, across);

    let along = s / SLAB;
    let joint = smoothstep(0.0, 0.035, min(fract(along), 1.0 - fract(along)) * SLAB);
    let bed = stone.bed.rgb * (0.82 + 0.36 * hash(floor(along))) * mix(0.55, 1.0, joint);
    let surface = pbr_input.material.base_color.rgb;
    pbr_input.material.base_color = vec4<f32>(mix(bed, stone.strip.rgb, strip) * surface, 1.0);
    pbr_input.material.metallic = strip * 0.5;
    pbr_input.material.perceptual_roughness = mix(0.85, 0.45, strip);

    var color = apply_pbr_lighting(pbr_input);
    color = vec4<f32>(color.rgb + inlay.hue.rgb * inlay.hue.a * lit * strip, color.a);
    out.color = main_pass_post_lighting_processing(pbr_input, color);
    return out;
}
