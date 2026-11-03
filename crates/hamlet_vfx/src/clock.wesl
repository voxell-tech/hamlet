// The scene's time in a material's shaders, in its main pass and its
// prepass alike: the view's own time, which runs every frame without the
// material changing, and `NowOffset` in `lib.rs`, which the material holds.

import bevy_render::globals::Globals;
@if(!PREPASS_PIPELINE) import bevy_pbr::render::mesh_view_bindings::globals;

// Where the prepass binds the view's time.
@if(PREPASS_PIPELINE)
@group(0) @binding(1) var<uniform> prepass_globals: Globals;

// The scene's time, `NowOffset` being `offset`.
fn scene_time(offset: f32) -> f32 {
@if(PREPASS_PIPELINE) {
    return prepass_globals.time + offset;
}
@else {
    return globals.time + offset;
}
}
