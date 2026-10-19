// A loop with no count, which the host refuses.
fn shade(surface: Surface) -> vec4<f32> {
    var x = surface.uv.x;
    while x < 1.0 { x = x * 1.5; }
    return vec4<f32>(x);
}
