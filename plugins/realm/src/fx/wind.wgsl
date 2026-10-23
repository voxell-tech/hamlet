// A gust's line (wind.rs): a soft dash that runs along its ribbon,
// faint behind, brightest near its front, clear at both ends and at
// both edges, so the ribbon past it draws nothing. UV.x runs along
// the ribbon and UV.y across it.
// Params: 0 the line's colour and how much of it shows; 1 where the
// dash's front is along the ribbon (x) and the share of the ribbon it
// covers (y).

fn shade(surface: Surface) -> vec4<f32> {
    let dash = params[1].y;
    let along = clamp((surface.uv.x - params[1].x + dash) / dash, 0.0, 1.0);
    let edge = pow(max(1.0 - abs(surface.uv.y * 2.0 - 1.0), 0.0), 0.8);
    let body = pow(along, 1.6) * pow(1.0 - along, 0.35);
    let shown = clamp(body * 2.2 * edge, 0.0, 1.0);
    return vec4<f32>(params[0].rgb, params[0].a * shown);
}
