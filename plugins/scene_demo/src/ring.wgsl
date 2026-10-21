// Bands that climb the ring over time, between two colours: params 0
// and 1.
fn shade(surface: Surface) -> vec4<f32> {
    let band = 0.5 + 0.5 * sin(surface.position.y * 12.0 + surface.time * 3.0);
    return vec4<f32>(mix(params[0].rgb, params[1].rgb, band), 1.0);
}
