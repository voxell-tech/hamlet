// A flag in the wind: cloth that hangs from its hoist at x = 0 and is
// blown along x. UV.x is how far a point is along the cloth, 0 at the
// hoist and 1 at the fly. Params: 0 how fast the waves run (x), how far
// the fly end swings as a share of the cloth's length (y), the waves
// along it (z) and its length (w); 1 its own moment of the wave (x).

fn displace(vertex: Vertex) -> vec3<f32> {
    let u = vertex.uv.x;
    let long = max(params[0].w, 0.001);
    let swing = params[0].y * long;
    let turn = 6.2832 * params[0].z;
    let a = turn * u - params[0].x * vertex.time + params[1].x;
    let b = a * 1.7 + 1.3;
    // The hoist is held; the cloth is freer the further along it is.
    // It lifts and shortens a little as it swings out.
    return vec3<f32>(
        -swing * 0.35 * u * u * (1.0 - cos(a)) * 0.5,
        swing * 0.22 * u * cos(a * 0.5),
        swing * u * (sin(a) + 0.35 * sin(b)),
    );
}

fn shade(surface: Surface) -> vec4<f32> {
    return surface.color;
}
