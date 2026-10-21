// Each cube of the field lifted by its item of `data`, which the
// waves compute writes, and coloured from low, params 0, to high,
// params 1. The cube whose tag is params 2.x is in params 3.
fn displace(vertex: Vertex) -> vec3<f32> {
    return data[vertex.tag].xyz;
}

fn shade(surface: Surface) -> vec4<f32> {
    let lift = clamp(data[surface.tag].y * 2.0 + 0.5, 0.0, 1.0);
    var color = mix(params[0].rgb, params[1].rgb, lift);
    if f32(surface.tag) == params[2].x {
        color = params[3].rgb;
    }
    return vec4<f32>(color, 1.0);
}
