// The view darkened toward its edges: from the distance params 0.x to
// params 0.y off its middle, by up to params 0.z.
fn post(pass: Pass) -> vec4<f32> {
    let off = distance(pass.uv, vec2<f32>(0.5));
    let kept = 1.0 - smoothstep(params[0].x, params[0].y, off) * params[0].z;
    return vec4<f32>(pass.color.rgb * kept, pass.color.a);
}
