//! What a plugin may spend: time per call, memory and tables. The
//! values are in `assets/wasm.toml`.

use std::fmt;
use std::time::Duration;

use bevy::prelude::*;
use hamlet_assets::{File, Tuning, designed, follow};
use wasmtime::ResourceLimiter;

use crate::reload::Polling;

/// The limits no grant changes.
#[derive(Resource, Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    /// The time one call may take, in milliseconds.
    pub call_ms: u64,
    /// The work one call may do, in wasmtime's fuel (about one unit
    /// for each instruction). Unlike the time, it does not depend on
    /// how busy the machine is.
    pub call_fuel: u64,
    /// The interval between two ticks of the engine's epoch. A
    /// call's time is counted in these.
    pub tick_ms: u64,
    /// The time all of one plugin's calls may take in a second, in
    /// milliseconds: its steps, its panels, its answers to calls and
    /// to the agents' turns, with what the host does for them. A
    /// plugin past it waits for the next second.
    pub plugin_ms_per_second: u64,
    /// The fuel all of one plugin's calls may use in a second.
    pub plugin_fuel_per_second: u64,
    /// The time the steps and panels of all the plugins may take in
    /// one frame, in milliseconds. The plugins left out go first in
    /// the next frame.
    pub frame_ms: u64,
    /// The ticks one plugin that wants them is given in a second at
    /// most. None at all when it is 0.
    pub tick_hz: u32,
    /// The times the host starts a plugin that trapped again by
    /// itself, before it waits for the chief.
    pub restart_retries: u32,
    /// How long a plugin that trapped waits before the host starts
    /// it again, in milliseconds.
    pub restart_backoff_ms: u64,
    /// The events one plugin may publish in a second. The rest are
    /// dropped.
    pub publishes_per_second: u32,
    /// The bytes of the payloads of the events one `step` may
    /// publish. The rest are dropped.
    pub publish_bytes_per_step: usize,
    /// The memory one plugin may grow to, in MiB.
    pub memory_mib: usize,
    /// The bytes of a plugin's `plugin.wasm`. A bigger one is not
    /// compiled.
    pub wasm_bytes: usize,
    /// The bytes the host may copy out of a plugin's memory for one
    /// crossing: the values given to one host function, or returned
    /// by one call of an export. Strings and lists that point at the
    /// same bytes count each time. A crossing over it traps the
    /// call.
    pub lift_bytes: usize,
    /// The elements one plugin's tables may hold.
    pub table_elements: usize,
    /// The core instances one plugin's component may hold.
    pub plugin_instances: u32,
    /// The memories one plugin's component may hold. Together they
    /// may grow to `memory_mib`.
    pub plugin_memories: u32,
    /// The tables one plugin's component may hold.
    pub plugin_tables: u32,
    /// The plugins the engine's pool holds at once, each with room
    /// for its instances, memories and tables. A plugin being
    /// reloaded holds two places while both versions run.
    pub pool_plugins: u32,
    /// The log lines one call may leave. The rest are dropped.
    pub log_lines: usize,
    /// The bytes of one log line. The rest of a longer line is cut.
    pub log_bytes: usize,
    /// The panels one plugin may declare. The rest are dropped.
    pub panels: usize,
    /// The nodes a panel's tree may hold.
    pub tree_nodes: usize,
    /// The shapes all the canvases of a panel's tree may hold. A
    /// tree with more fails the call.
    pub canvas_shapes: usize,
    /// The width and the height of a canvas, at most, in pixels.
    pub canvas_px: u32,
    /// The levels a panel's tree may nest.
    pub tree_depth: usize,
    /// The bytes of one text in a panel's tree: a node's text, label
    /// or id. A tree with a longer one fails the call.
    pub text_bytes: usize,
    /// The bytes of all the text in a panel's tree, or of the
    /// program a panel returns. A tree with more fails the call.
    pub tree_text_bytes: usize,
    /// The events from the panels that one plugin is given in a
    /// second. The rest are dropped.
    pub events_per_second: u32,
    /// The intents one `step` may return. The rest are dropped.
    pub intents_per_call: usize,
    /// The bytes of all the values one plugin keeps in its storage.
    pub storage_bytes: u64,
    /// The keys one plugin may keep in its storage.
    pub storage_keys: usize,
    /// The bytes of one storage key.
    pub storage_key_bytes: usize,
    /// The bytes of one storage value.
    pub storage_value_bytes: usize,
    /// The acts as the chief, messages and commands together, that
    /// one plugin may do in a minute. The rest are dropped.
    pub acts_per_minute: u32,
    /// The bytes of the text of one message a plugin sends as the
    /// chief.
    pub act_text_bytes: usize,
    /// The time one fetch may take, in milliseconds.
    pub fetch_ms: u64,
    /// The bytes of the body of a request a plugin fetches with.
    pub fetch_request_bytes: usize,
    /// The bytes of the body of an answer to a fetch.
    pub fetch_response_bytes: usize,
    /// The fetches one plugin may have waiting at once.
    pub fetches_in_flight: usize,
    /// The fetches one plugin may start in a minute.
    pub fetches_per_minute: u32,
    /// The bytes of all the files one plugin keeps in its folder of
    /// files.
    pub files_bytes: u64,
    /// The files and folders one plugin may keep in its folder of
    /// files.
    pub files_count: usize,
    /// The bytes of one file.
    pub file_bytes: usize,
    /// The bytes of the path of one file.
    pub file_path_bytes: usize,
    /// The bytes of the title a plugin gives the dialog a file is
    /// picked in. A longer one is cut.
    pub pick_title_bytes: usize,
    /// The dialogs one plugin may open in a minute.
    pub picks_per_minute: u32,
    /// How long a plugin whose dialog the chief closed with nothing
    /// waits before it may open another, in milliseconds.
    pub pick_rest_ms: u64,
    /// The bytes of one file a plugin reads or writes in the folder
    /// the chief chose for it.
    pub folder_file_bytes: usize,
    /// The names one list of a folder the chief chose gives at most.
    pub folder_list_entries: usize,
    /// The bytes of a path in the folder the chief chose.
    pub folder_path_bytes: usize,
    /// The calls in the folder the chief chose that one plugin may
    /// have waiting at once, a write the chief is asked about among
    /// them.
    pub folder_calls_in_flight: usize,
    /// The writes in the folder the chief chose that one plugin may
    /// put to the chief in a minute. The rest are answered with an
    /// error.
    pub folder_asks_per_minute: u32,
    /// How long a plugin whose write the chief turned down waits
    /// before it may put another, in milliseconds.
    pub folder_rest_ms: u64,
    /// The model calls one plugin may make in a minute.
    pub model_calls_per_minute: u32,
    /// The model calls one plugin may have waiting at once.
    pub model_calls_in_flight: usize,
    /// The tokens of one model call, what it sends and the most its
    /// reply may have, counted as four characters a token. A call
    /// over it is refused.
    pub model_call_tokens: u64,
    /// The tokens of the reply to one model call, at most. A reply
    /// is cut off there.
    pub model_reply_tokens: u32,
    /// The bytes of all the tables one plugin keeps in its database.
    pub db_bytes: u64,
    /// The tables one plugin may keep in its database.
    pub db_tables: usize,
    /// The records one table may hold.
    pub db_records: usize,
    /// The bytes of one record: its id, its bytes and its fields.
    pub db_record_bytes: usize,
    /// The indexed fields of one record.
    pub db_fields: usize,
    /// The bytes of the name of a table, and of a field.
    pub db_name_bytes: usize,
    /// The bytes of the id of a record, and of the value of a field.
    pub db_text_bytes: usize,
    /// The records of one page of a list or a find.
    pub db_page_records: usize,
    /// The bytes of the records of one page. A page holds at least
    /// one record, whatever its size.
    pub db_page_bytes: usize,
    /// The bytes of the state the host holds for one plugin.
    pub state_bytes: usize,
    /// The commands one plugin may declare. The rest are dropped.
    pub commands: usize,
    /// The menu lines one plugin may declare. The rest are dropped.
    pub menu_items: usize,
    /// The context menu items one plugin may declare. The rest are
    /// dropped.
    pub context_items: usize,
    /// The inspector tabs one plugin may declare. The rest are
    /// dropped.
    pub inspector_sections: usize,
    /// The settings pages one plugin may declare. The rest are
    /// dropped.
    pub settings_pages: usize,
    /// The shortcuts one plugin may declare. The rest are dropped.
    pub keys: usize,
    /// The tools one plugin may give the agents. The rest are
    /// dropped.
    pub agent_tools: usize,
    /// The fields of one tool. The rest are dropped.
    pub tool_fields: usize,
    /// The bytes of what a tool returns to an agent. A longer text
    /// is cut.
    pub tool_result_bytes: usize,
    /// The time the `step` that answers a point of an agent's turn
    /// may take, in milliseconds. The turn waits for it.
    pub steer_ms: u64,
    /// The work the `step` that answers a point of an agent's turn
    /// may do, in wasmtime's fuel.
    pub steer_fuel: u64,
    /// The bytes of a note or of the reason for a hold a plugin that
    /// steers an agent gives. A longer one is cut.
    pub steer_note_bytes: usize,
    /// The bytes of the arguments of a tool call a plugin that
    /// steers an agent is shown. Longer ones are cut.
    pub steer_args_bytes: usize,
    /// The calls of agents one plugin may have waiting at once.
    pub tool_calls_in_flight: usize,
    /// The plugins a call may cross, one running a command whose
    /// handler runs another. A call over it is answered with an
    /// error.
    pub call_depth: usize,
    /// The calls one plugin may have waiting for an answer at once.
    pub calls_in_flight: usize,
    /// The frames a call waits for its answer before it is answered
    /// with an error.
    pub call_frames: u64,
    /// The bytes of the arguments of a call, and of a reply.
    pub call_bytes: usize,
    /// The time the `step` that is given a call of an interface may
    /// take, in milliseconds.
    pub invoke_ms: u64,
    /// The work the `step` that is given a call of an interface may
    /// do, in wasmtime's fuel. The caller pays for it.
    pub invoke_fuel: u64,
    /// The fuel the calls of interfaces one plugin makes may use in
    /// a second. A call over it is answered with an error.
    pub invoke_fuel_per_second: u64,
    /// The items of one page of a collection another plugin shares.
    pub collection_page_items: usize,
    /// The bytes of the values of one page. A page holds at least
    /// one item, whatever its size.
    pub collection_page_bytes: usize,
    /// The hamlets one read of the world gives at most.
    pub world_hamlets: usize,
    /// The agents one read of the world gives at most.
    pub world_agents: usize,
    /// The marks one plugin may draw over the world.
    pub overlay_marks: usize,
    /// The bytes of the words and the names of anchors of one
    /// plugin's marks over the world.
    pub overlay_text_bytes: usize,
    /// The times one plugin may replace its marks over the world in
    /// a second. The rest are dropped.
    pub overlay_updates_per_second: u32,
    /// The nodes of one plugin's scene in the world.
    pub scene_nodes: usize,
    /// The lights of one plugin's scene.
    pub scene_lights: usize,
    /// The levels the tree of a scene may nest.
    pub scene_depth: usize,
    /// The radius in metres about a scene's site that its shapes and
    /// lights stay within.
    pub scene_radius: f32,
    /// The times one plugin may replace its scene in a second. A
    /// change sent sooner waits, and only the last is kept.
    pub scene_updates_per_second: u32,
    /// The brightest light of a scene, in lumens.
    pub scene_lumens: f32,
    /// The farthest a light of a scene reaches, in metres.
    pub scene_light_range: f32,
    /// The strongest glow of a scene's surface.
    pub scene_glow: f32,
    /// The meshes one plugin may send.
    pub scene_meshes: usize,
    /// The points of one mesh.
    pub scene_mesh_points: usize,
    /// The triangles of one mesh.
    pub scene_mesh_triangles: usize,
    /// The shaders one plugin may send.
    pub shaders: usize,
    /// The bytes of one shader's source.
    pub shader_bytes: usize,
    /// The times the loops of a shader may run for one point,
    /// counted as the product of the loops one is in.
    pub shader_loop_runs: u64,
}

/// The time one call may take, in milliseconds, for runs on a busy
/// machine: the tests and the harness.
pub const PATIENT_MS: u64 = 10_000;

impl Limits {
    /// These limits with a call time that a busy machine does not
    /// pass.
    pub fn patient(self) -> Self {
        Self {
            call_ms: PATIENT_MS,
            steer_ms: PATIENT_MS,
            invoke_ms: PATIENT_MS,
            plugin_ms_per_second: PATIENT_MS,
            frame_ms: PATIENT_MS,
            ..self
        }
    }

    /// These limits with the time of a `step` that is given a call of
    /// an interface, and `fuel`.
    pub fn invoking(&self, fuel: u64) -> Self {
        Self {
            call_ms: self.invoke_ms,
            call_fuel: fuel,
            ..self.clone()
        }
    }

    /// These limits with the time and fuel of a `step` that answers
    /// a point of an agent's turn.
    pub fn steering(&self) -> Self {
        Self {
            call_ms: self.steer_ms,
            call_fuel: self.steer_fuel,
            ..self.clone()
        }
    }

    pub fn tick(&self) -> Duration {
        Duration::from_millis(self.tick_ms)
    }

    /// The ticks a call may last, at least one.
    pub fn call_ticks(&self) -> u64 {
        self.call_ms.div_ceil(self.tick_ms).max(1)
    }

    pub fn memory_bytes(&self) -> usize {
        self.memory_mib << 20
    }
}

/// The sections of `assets/wasm.toml`.
#[derive(Asset, TypePath, Clone, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WasmTuning {
    pub limits: Limits,
    pub reload: Polling,
}

impl Tuning for WasmTuning {
    const FILE: File = hamlet_assets::WASM;

    fn apply(&self, was: &Self, world: &mut World) {
        follow(world, &was.limits, &self.limits);
        follow(world, &was.reload, &self.reload);
    }
}

designed!(WasmTuning { Limits => limits, Polling => reload });

/// Why a plugin's memory or table could not grow.
#[derive(Debug)]
pub struct OverLimit(pub &'static str);

impl fmt::Display for OverLimit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the plugin went over its {} limit", self.0)
    }
}

impl std::error::Error for OverLimit {}

/// Holds one plugin's store to its [`Limits`]: the memory of all its
/// memories together, each table, and how many of each it makes. A
/// growth past a limit traps the call.
pub struct Limiter {
    memory: usize,
    /// The bytes of all the store's memories.
    used: usize,
    /// The last growth allowed, taken back if it failed.
    growing: usize,
    table_elements: usize,
    instances: usize,
    memories: usize,
    tables: usize,
}

impl Limiter {
    pub fn new(limits: &Limits) -> Self {
        Self {
            memory: limits.memory_bytes(),
            used: 0,
            growing: 0,
            table_elements: limits.table_elements,
            instances: limits.plugin_instances as usize,
            memories: limits.plugin_memories as usize,
            tables: limits.plugin_tables as usize,
        }
    }
}

impl ResourceLimiter for Limiter {
    fn memory_growing(
        &mut self,
        current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        let growth = desired.saturating_sub(current);
        if self.used.saturating_add(growth) > self.memory {
            return Err(wasmtime::Error::new(OverLimit("memory")));
        }
        self.used += growth;
        self.growing = growth;
        Ok(true)
    }

    fn memory_grow_failed(
        &mut self,
        _error: wasmtime::Error,
    ) -> wasmtime::Result<()> {
        self.used -= std::mem::take(&mut self.growing);
        Ok(())
    }

    fn table_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        if desired > self.table_elements {
            return Err(wasmtime::Error::new(OverLimit("table")));
        }
        Ok(true)
    }

    fn instances(&self) -> usize {
        self.instances
    }

    fn memories(&self) -> usize {
        self.memories
    }

    fn tables(&self) -> usize {
        self.tables
    }
}
