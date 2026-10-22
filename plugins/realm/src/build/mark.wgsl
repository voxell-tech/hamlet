// A mark of light on the ground (mark.rs): a ring, or a rune of
// straight sides, worked out for each pixel, so that its edge is as
// sharp near as far. It is painted only where what is seen is the
// ground: within `near` of the height the world gives the ground at
// that spot. A wall, a robot or a barrel stands higher than the ground
// under it and is left alone.
// Params: 0 the light, in linear RGB, and how strong it is; 1 the
// middle of the ground the heights are of, as x and z, how far it
// reaches to each side, and the height that 0 stands for; 2 how many
// sides the outline has, none for a ring, the sizes its line lies
// between, where a shape of size 1 touches the square's sides, and
// how far over or under the ground still counts as the ground; 3 the
// points along each side of the heights (x).
// Data: the heights over the ground's middle, row by row along z, four
// to an item.

// How much of the outline covers the point `uv` of the square, each
// edge soft over one pixel.
fn outline(uv: vec2<f32>) -> f32 {
    let p = (uv - 0.5) * 2.0;
    let shape = params[2];
    // How large a shape of this kind is that passes through the point.
    var size = length(p);
    if shape.x >= 3.0 {
        let part = 6.2831853 / shape.x;
        let around = atan2(p.y, p.x);
        let within = around - part * floor(around / part) - part * 0.5;
        size = size * cos(within) / cos(part * 0.5);
    }
    // No wider than a fiftieth, where a step in what is seen makes the
    // size leap from one pixel to the next.
    let soft = clamp(fwidth(size), 0.0005, 0.02) * 0.5;
    return smoothstep(shape.y - soft, shape.y + soft, size)
        * (1.0 - smoothstep(shape.z - soft, shape.z + soft, size));
}

fn height(x: i32, y: i32, side: i32) -> f32 {
    let i = y * side + x;
    return data[i / 4][i % 4];
}

// The height of the ground at `at`, between the four heights about it.
fn ground(at: vec2<f32>) -> f32 {
    let field = params[1];
    let side = i32(params[3].x);
    let size = vec2<f32>(f32(side));
    let uv = (at - field.xy) / (field.z * 2.0) + 0.5;
    let p = clamp(uv * size - 0.5, vec2<f32>(0.0), size - 1.0);
    let low = vec2<i32>(floor(p));
    let high = min(low + 1, vec2<i32>(side - 1));
    let f = fract(p);
    let a = height(low.x, low.y, side);
    let b = height(high.x, low.y, side);
    let c = height(low.x, high.y, side);
    let d = height(high.x, high.y, side);
    return field.w + mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}

fn shade(decal: Decal) -> vec4<f32> {
    let drawn = outline(decal.uv);
    let near = params[2].w;
    let off = abs(decal.position.y - ground(decal.position.xz));
    // Fades over the last third, so that the edge of a step is soft.
    let on_ground = 1.0 - smoothstep(near * 0.67, near, off);
    return vec4<f32>(params[0].rgb, drawn * on_ground * params[0].a);
}
