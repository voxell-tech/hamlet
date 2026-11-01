// A wire between two ports (wire.rs): a cubic Bezier curve drawn over
// a node that covers it. For every pixel the distance to the curve is
// found, from the curve cut into short straight pieces, and measured in
// screen pixels, so the line is as thick and as smooth at any zoom.

import bevy_ui_render::ui_vertex_output::UiVertexOutput;

struct Wire {
    // Its colour, in linear RGB, and how opaque it is.
    colour: vec4<f32>,
    // Where it starts and where it ends, as x and y in the node, in
    // the same units as `size`.
    ends: vec4<f32>,
    // The curve's two handles, the one by the start first.
    handles: vec4<f32>,
    // The node's width and height, and half the line's thickness in
    // screen pixels.
    size: vec4<f32>,
}

@group(1) @binding(0) var<uniform> wire: Wire;

// Pieces the curve is cut into to measure the distance to it.
const PIECES: i32 = 32;

fn at(t: f32) -> vec2<f32> {
    let u = 1.0 - t;
    return wire.ends.xy * (u * u * u)
        + wire.handles.xy * (3.0 * u * u * t)
        + wire.handles.zw * (3.0 * u * t * t)
        + wire.ends.zw * (t * t * t);
}

fn to_piece(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
    let ab = b - a;
    let along = clamp(dot(p - a, ab) / max(dot(ab, ab), 1e-6), 0.0, 1.0);
    return length(p - a - ab * along);
}

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    let p = in.uv * wire.size.xy;
    // How much of the node one screen pixel spans, whatever the zoom.
    let pixel = max(length(fwidth(p)) * 0.7071, 1e-4);
    var near = 1e9;
    var last = at(0.0);
    for (var i = 1; i <= PIECES; i++) {
        let next = at(f32(i) / f32(PIECES));
        near = min(near, to_piece(p, last, next));
        last = next;
    }
    let off = near / pixel;
    let half = wire.size.z;
    let cover = 1.0 - smoothstep(half - 0.75, half + 0.75, off);
    return vec4<f32>(wire.colour.rgb, wire.colour.a * cover);
}
