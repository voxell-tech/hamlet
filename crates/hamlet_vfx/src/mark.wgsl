// A mark of light on the ground (mark.rs). It is drawn through a box
// that stands over the place, whose far sides cover on the screen all
// the ground it could fall on, whatever the angle. For every pixel of
// it, what the camera sees there is found from the depth prepass, and
// the mark is laid out on a square one unit wide at the box's middle,
// seen from above. It is
// painted only where that is the ground: within `near` of the height
// the world says the ground has at that spot. A wall, a robot or a
// barrel stands higher than the ground under it and is left alone.

#import bevy_pbr::{
    forward_io::VertexOutput,
    mesh_functions::get_local_from_world,
    prepass_utils::prepass_depth,
    view_transformations::{frag_coord_to_ndc, position_ndc_to_world},
}

struct Mark {
    // The light, in linear RGB, and how strong it is.
    light: vec4<f32>,
    // The middle of the ground the heights are of, as x and z, how
    // far it reaches to each side, and the height that 0 stands for.
    field: vec4<f32>,
    // The outline that is drawn: how many sides it has, none for a
    // ring, and the sizes it lies between, where a shape of size 1
    // touches the square's sides.
    shape: vec4<f32>,
    // How far over or under the ground still counts as the ground.
    near: f32,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> mark: Mark;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var heights: texture_2d<f32>;

// How much of the outline covers the point `uv` of the square. It is
// worked out and not looked up in a picture, so its edge is as sharp
// near as far: soft over one pixel, whatever the size of the mark.
fn outline(uv: vec2<f32>) -> f32 {
    let p = (uv - 0.5) * 2.0;
    // How large a shape of this kind is that passes through the
    // point.
    var size = length(p);
    if mark.shape.x >= 3.0 {
        let part = 6.2831853 / mark.shape.x;
        let around = atan2(p.y, p.x);
        let within = around - part * floor(around / part) - part * 0.5;
        size = size * cos(within) / cos(part * 0.5);
    }
    // No wider than a fiftieth, where a step in what is seen makes
    // the size leap from one pixel to the next.
    let soft = clamp(fwidth(size), 0.0005, 0.02);
    return smoothstep(mark.shape.y - soft, mark.shape.y + soft, size)
        * (1.0 - smoothstep(mark.shape.z - soft, mark.shape.z + soft, size));
}

// The height of the ground at `at`, between the four heights about
// it.
fn ground(at: vec2<f32>) -> f32 {
    let size = vec2<f32>(textureDimensions(heights));
    let uv = (at - mark.field.xy) / (mark.field.z * 2.0) + 0.5;
    let p = clamp(uv * size - 0.5, vec2(0.0), size - 1.0);
    let low = vec2<i32>(floor(p));
    let high = min(low + 1, vec2<i32>(size) - 1);
    let f = fract(p);
    let a = textureLoad(heights, low, 0).r;
    let b = textureLoad(heights, vec2(high.x, low.y), 0).r;
    let c = textureLoad(heights, vec2(low.x, high.y), 0).r;
    let d = textureLoad(heights, high, 0).r;
    return mark.field.w + mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
#ifdef DEPTH_PREPASS
    let depth = prepass_depth(in.position, 0u);
    let seen = position_ndc_to_world(
        frag_coord_to_ndc(vec4(in.position.xy, depth, 1.0)),
    );
    // Where that lies on the square: its middle is the origin and
    // it is one unit wide.
    let local = get_local_from_world(in.instance_index) * vec4(seen, 1.0);
    let uv = local.xz + 0.5;
    // Before anything is left out: its edge is found from how it
    // changes between pixels.
    let drawn = outline(uv);
    if any(uv < vec2(0.0)) || any(uv > vec2(1.0)) {
        discard;
    }
    let off = abs(seen.y - ground(seen.xz));
    // Fades over the last third, so that the edge of a step is soft.
    let on_ground = 1.0 - smoothstep(mark.near * 0.67, mark.near, off);
    let alpha = drawn * on_ground * mark.light.a;
    if alpha < 0.004 {
        discard;
    }
    // Light is added to what is there, so a dimmer mark is a
    // fainter one and never a dark one.
    return vec4(mark.light.rgb * alpha, 0.0);
#else
    // Without the depth of what it falls on, it is drawn on the box
    // as seen from above.
    let flat = get_local_from_world(in.instance_index) * in.world_position;
    return vec4(mark.light.rgb * outline(flat.xz + 0.5) * mark.light.a, 0.0);
#endif
}
