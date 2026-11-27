// A surface that pulses between two colours over time.

import bevy_pbr::render::forward_io::VertexOutput;
import bevy_pbr::render::mesh_view_bindings::globals;

struct Pulse {
    low: vec4<f32>,
    high: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> pulse: Pulse;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let wave = 0.5 + 0.5 * sin(globals.time * 3.0 + in.uv.y * 12.0);
    return mix(pulse.low, pulse.high, wave);
}
