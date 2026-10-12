// The light that falls into a panel: lamplight from over its top while
// something waits there, and a breath of colour when what it shows changes.
// A panel at rest draws nothing. Drawn over the panel's fill and under its
// contents. Colours are linear and distances are logical pixels.

#import bevy_render::globals::Globals
#import bevy_ui::ui_vertex_output::UiVertexOutput

@group(0) @binding(1) var<uniform> globals: Globals;

// rgb: the lamplight. a: how far it is turned up, over 1 as it is first lit.
@group(1) @binding(0) var<uniform> alarm: vec4<f32>;
// rgb: colour. a: strength, 0 to 1.
@group(1) @binding(1) var<uniform> flash: vec4<f32>;
// rgb: the heart of the flame.
@group(1) @binding(2) var<uniform> heart: vec4<f32>;
// x: the share of its light the flame wavers by. w: physical pixels to a
// logical one.
@group(1) @binding(3) var<uniform> shape: vec4<f32>;

// Light from a lamp hung just over the panel's top, a third of the way
// along, that reaches `deep` pixels down.
fn pool(p: vec2<f32>, size: vec2<f32>, deep: f32) -> f32 {
    let off = (p - vec2<f32>(size.x * 0.32, -18.0)) / vec2<f32>(size.x * 0.7, deep);
    return exp(-dot(off, off) * 1.6);
}

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    let size = in.size / shape.w;
    let p = in.uv * size;
    let t = globals.time;
    let inside = min(p, size - p);
    let edge = min(inside.x, inside.y);

    // A flame is never quite still.
    let waver = 1.0 + shape.x * (0.5 * sin(t * 5.3) + 0.3 * sin(t * 8.9 + 1.7) + 0.2 * sin(t * 2.1 + 0.6));
    let call = alarm.a * waver;
    let glow = pool(p, size, 240.0) * call;
    // The frame catches the light, most where it is nearest the lamp.
    let line = (1.0 - smoothstep(0.5, 1.5, edge)) * (0.2 + 0.8 * pool(p, size, 460.0)) * call;
    // Brightest close under the lamp, and thin over the rest.
    let lamp = clamp(glow * 0.13 + glow * glow * 0.14 + line * 0.6, 0.0, 1.0);
    let lit = mix(alarm.rgb, heart.rgb, clamp(glow * 0.35, 0.0, 1.0));

    let breath = flash.a * flash.a * pool(p, size, 130.0) * 0.22;

    let alpha = clamp(lamp + breath, 0.0, 1.0);
    let colour = (lit * lamp + flash.rgb * breath) / max(lamp + breath, 0.0001);
    return vec4<f32>(colour, alpha);
}
