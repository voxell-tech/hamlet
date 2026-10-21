// The lift of each cube of a square field: params 0 holds the count,
// the side and the height of the waves.
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let count = u32(params[0].x);
    let side = max(u32(params[0].y), 1u);
    if id.x >= count {
        return;
    }
    let x = f32(id.x % side);
    let z = f32(id.x / side);
    let wave = sin(x * 0.6 + frame.time * 2.0) * cos(z * 0.6 + frame.time * 1.3);
    data[id.x] = vec4<f32>(0.0, wave * params[0].z, 0.0, 0.0);
}
