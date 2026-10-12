// The light of a control. On the control itself it is a flame that sits
// low in the glass and lights the fill from below, drawn over the fill and
// under the lettering. On a node that reaches past the control on every
// side it is the light that spills out by the foot. Colours are
// linear and distances are physical pixels.

#import bevy_ui::ui_vertex_output::UiVertexOutput

// rgb: the light. a: how bright, 0 to 1.
@group(1) @binding(0) var<uniform> hue: vec4<f32>;
// rgb: the heart of the flame. a: how much of the glass shows when the lamp
// is out, 0 to 1.
@group(1) @binding(1) var<uniform> heart: vec4<f32>;
// The glass away from the flame and near it, rgb.
@group(1) @binding(2) var<uniform> low: vec4<f32>;
@group(1) @binding(3) var<uniform> high: vec4<f32>;
// x: how far the node reaches past the control, 0 on the control itself.
// y: the control's corner radius. z: the side the flame leans to, -1 to 1.
// w: 1 when the light is that of lettering with no plate, which burns
// behind the words too and as much above them as under.
@group(1) @binding(4) var<uniform> shape: vec4<f32>;

// The distance of `p` outside a box of `half` extents with round corners.
fn outside(p: vec2<f32>, half: vec2<f32>, radius: f32) -> f32 {
    let q = abs(p) - half + vec2<f32>(radius);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - radius;
}

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    let reach = shape.x;
    let p = (in.uv - 0.5) * in.size;
    let half = in.size * 0.5 - vec2<f32>(reach);
    let away = outside(p, half, shape.y);
    // The flame sits at the foot, a little to one side.
    let seat = vec2<f32>(shape.z * half.x * 0.18, half.y * 1.05);
    let off = p - seat;

    if reach <= 0.0 {
        // Inside the frame, which is left as it is.
        let frame = in.border_widths.x;
        let cover = clamp(-away - frame + 0.5, 0.0, 1.0);
        // Wide and tall enough to fade out inside the glass, with no rim.
        let q = off / vec2<f32>(half.x * 1.5, half.y * 1.6);
        let pool = exp(-dot(q, q) * 2.0);
        let glass = mix(low.rgb, high.rgb, pool * (0.55 + 0.45 * hue.a));
        let lit = mix(glass, heart.rgb, pool * hue.a * 0.4);
        let bare = mix(hue.rgb, heart.rgb, pool * 0.5);
        let light = hue.a * pool * 0.8;
        let colour = mix(bare, lit, heart.a);
        let alpha = mix(light, 0.97, heart.a);
        return vec4<f32>(colour, alpha * cover);
    }

    let ink = shape.w;
    if away <= 0.0 && ink < 0.5 {
        return vec4<f32>(0.0);
    }
    if ink >= 0.5 {
        // Brightest behind the words, and fading evenly from them.
        let out = max(away, 0.0);
        let fade = exp(-out / (reach * 0.5));
        let gone = 1.0 - smoothstep(reach * 0.55, reach, out);
        return vec4<f32>(hue.rgb, hue.a * 0.55 * fade * gone);
    }
    // Soft from the first pixel, and gone before the node's own edge.
    let fall = exp(-away / (reach * 0.42));
    let end = 1.0 - smoothstep(reach * 0.5, reach, away);
    // It falls under the control, as on a table under a lamp, and next to
    // none leaves by the top or the ends.
    let wide = off.x / (half.x * 1.1 + reach);
    let side = exp(-wide * wide * 1.3);
    let lean = 0.06 + 0.94 * smoothstep(-half.y * 0.4, half.y, p.y);
    return vec4<f32>(hue.rgb, hue.a * fall * end * side * lean);
}
