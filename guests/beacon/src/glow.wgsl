// Bands of light that rise up the orb, coloured by its first two values
// and quickened by the third.
fn shade(surface: Surface) -> vec4<f32> {
    let core = param(0u).rgb;
    let rim = param(1u).rgb;
    let speed = param(2u).x;
    var glow = 0.0;
    for (var i = 0; i < 3; i++) {
        let band = f32(i + 1);
        let wave = sin(surface.position.y * 6.0 * band
            - surface.time * speed * band);
        glow += (0.5 + 0.5 * wave) / band;
    }
    let facing = abs(dot(surface.normal, vec3<f32>(0.0, 1.0, 0.0)));
    return vec4<f32>(mix(rim, core, facing) * (0.6 + glow), 1.0);
}
