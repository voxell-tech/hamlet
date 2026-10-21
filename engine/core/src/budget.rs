//! What a plugin may spend, as plain data from
//! `engine/tuning/engine.toml`.

use bevy::ecs::resource::Resource;
use serde::Deserialize;

use crate::tuning::{ENGINE, Tuning, TuningFile};

/// The caps on one plugin.
#[derive(Resource, Clone, PartialEq, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Budgets {
    /// Bytes of memory, all of the plugin's memories summed.
    pub memory: u64,
    /// Fuel for one call into the plugin.
    pub fuel_per_call: u64,
    /// Milliseconds one call may take.
    pub call_time: f32,
    /// Milliseconds of calls in one second.
    pub time_per_second: f32,
    /// Fuel of calls in one second.
    pub fuel_per_second: u64,
    /// Seconds in a row a plugin may go over a budget of a second
    /// before it is stopped.
    pub strikes: u32,
    /// Milliseconds of each frame shared among the plugins.
    pub frame_share: f32,
    /// Bytes copied into or out of the plugin by one call, checked
    /// before any copy.
    pub bytes_per_call: u64,
    pub calls_per_second: u32,
    pub events_per_second: u32,
    /// Requests to the engine's services one call may make; past
    /// them a request is not taken.
    pub requests_per_call: usize,
    /// Resources of the system interface (WASI) it holds at once,
    /// such as streams and timers; past them one is refused.
    pub wasi_resources: usize,
    /// Assets made, changed or deleted in one second.
    pub asset_writes_per_second: u32,
    /// Calls of other plugins' services in one second.
    pub service_calls_per_second: u32,
    /// Fuel the services it called spent serving it in one second,
    /// charged to it.
    pub service_fuel_per_second: u64,
    /// Milliseconds the services it called took in one second,
    /// charged to it.
    pub service_time_per_second: f32,
    /// Plugins a chain of calls and events may cross.
    pub chain_depth: u32,
    /// Events it sends in one second; the rest are refused.
    pub events_sent_per_second: u32,
    pub fetches_per_minute: u32,
    /// Fetches under way at once.
    pub fetches_waiting: usize,
    /// Bytes of the answer to one fetch.
    pub fetch_bytes: u64,
    /// Seconds one fetch may take, its lookup with it.
    pub fetch_time: f32,
    pub panels: usize,
    /// Everything it adds: panels, commands, menu entries,
    /// shortcuts, settings pages and suggested workspaces.
    pub contributions: usize,
    /// Bytes of one asset. Less than `bytes_per_call`, so a read
    /// asset can be handed over.
    pub asset_bytes: u64,
    /// Assets it may own.
    pub assets: usize,
    pub log_lines_per_second: u32,
    /// Bytes of one line of the log; a longer line is cut.
    pub log_line_bytes: usize,
    pub petitions_per_minute: u32,
    /// Nodes of retained panels made, set or put in order in one
    /// call; past them the call fails.
    pub node_changes_per_call: usize,
    /// The same, in one second.
    pub node_changes_per_second: u32,
    /// Nodes of retained panels it holds at once; past them the call
    /// fails.
    pub nodes: usize,
    /// How deep a node of a retained panel may sit under its root;
    /// a call that makes one deeper fails.
    pub node_depth: usize,
    /// Entities of its scenes held at once, each copy of instances
    /// counted; past them a make fails.
    pub scene_entities: usize,
    /// Vertices of its meshes held at once.
    pub scene_vertices: usize,
    pub scene_materials: usize,
    pub scene_lights: usize,
    pub scene_cameras: usize,
    /// Changes to its scenes in one call; past them the call fails.
    pub scene_changes_per_call: usize,
    /// The same, in one second; past them the plugin waits.
    pub scene_changes_per_second: u32,
    /// Bytes of GPU memory its meshes, textures and buffers take, as
    /// the engine counts them; past them a make fails.
    pub gpu_memory: u64,
    /// Shaders, buffers, textures, compute and passes it holds at
    /// once.
    pub gpu_objects: usize,
    /// Bytes of one shader's source.
    pub shader_bytes: usize,
    /// Workgroups its compute runs in one frame; a dispatch past
    /// them is refused, and runs past them wait for the next
    /// frame.
    pub workgroups_per_frame: u32,
    /// Milliseconds of GPU time its compute and passes take in one
    /// second, where the GPU can time them; past them they wait for
    /// the next second.
    pub gpu_time_per_second: f32,
}

impl Tuning for Budgets {
    const FILE: TuningFile = ENGINE;
    const SECTION: Option<&'static str> = Some("budgets");
}
