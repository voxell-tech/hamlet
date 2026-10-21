//! The wasmtime engine every plugin runs on, and the thread that
//! ticks its epoch.

use std::thread;
use std::time::Duration;

use bevy::app::{App, Startup};
use bevy::ecs::message::MessageWriter;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Res};
use hamlet_engine_api::PluginLimits;
use hamlet_engine_core::tuning::{ENGINE, Tuning, TuningFile};
use hamlet_engine_core::{LogLine, TuningPlugin};
use serde::Deserialize;
use wasmtime::{
    Config, Engine, InstanceAllocationStrategy,
    PoolingAllocationConfig,
};

pub(crate) fn plugin(app: &mut App) {
    app.add_plugins(TuningPlugin::<WasmTuning>::default())
        .add_systems(Startup, start);
}

/// How the host runs plugins. The tick and the pool are read as the
/// engine starts.
#[derive(Resource, Clone, PartialEq, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WasmTuning {
    /// Milliseconds between two ticks of the epoch, which a call's
    /// time is counted in.
    pub tick: f32,
    /// Plugins that can run at once, each with its share of the
    /// pool. The pool holds one more, for a version built beside the
    /// running one.
    pub plugins: u32,
    /// Core instances one plugin's component may hold.
    pub instances: u32,
    /// Memories one plugin's component may hold. Together they hold
    /// the plugin's `memory` limit at most.
    pub memories: u32,
    pub tables: u32,
    /// Elements of one table.
    pub table_elements: usize,
    /// Bytes of a `plugin.wasm`; a larger one is not read.
    pub wasm_bytes: u64,
    /// Bytes of a `plugin.toml`; a larger one is not read.
    pub manifest_bytes: u64,
    /// Seconds between two looks at the plugins folder, read as the
    /// engine starts.
    pub watch: f32,
}

impl Tuning for WasmTuning {
    const FILE: TuningFile = ENGINE;
    const SECTION: Option<&'static str> = Some("wasm");
}

impl WasmTuning {
    /// Ticks of the epoch one call may last, at least one. wasmtime
    /// adds them to the epoch, so they stay far from `u64::MAX`.
    pub(crate) fn call_ticks(&self, budgets: &PluginLimits) -> u64 {
        (budgets.call_time / self.tick)
            .ceil()
            .clamp(1.0, u32::MAX as f32) as u64
    }
}

/// The engine every plugin runs on.
#[derive(Resource, Clone)]
pub struct Host(pub(crate) Engine);

impl Host {
    /// An engine for components with fuel and an epoch, and a pool
    /// in which each plugin's share is capped, so no plugin can take
    /// another's room. Its epoch is ticked for as long as it lives.
    pub fn new(tuning: &WasmTuning) -> wasmtime::Result<Self> {
        let slots = tuning.plugins + 1;
        let mut pool = PoolingAllocationConfig::new();
        // A memory's size is left to the pool's own most, all a
        // 32-bit memory can address, so each plugin's `memory` limit
        // holds it, whatever it is set to.
        pool.table_elements(tuning.table_elements)
            .max_core_instances_per_component(tuning.instances)
            .max_memories_per_component(tuning.memories)
            .max_memories_per_module(tuning.memories)
            .max_tables_per_component(tuning.tables)
            .max_tables_per_module(tuning.tables)
            .total_component_instances(slots)
            .total_core_instances(slots * tuning.instances)
            .total_memories(slots * tuning.memories)
            .total_tables(slots * tuning.tables);
        let mut config = Config::new();
        config
            .wasm_component_model(true)
            .consume_fuel(true)
            .epoch_interruption(true)
            .allocation_strategy(
                InstanceAllocationStrategy::Pooling(pool),
            );
        let engine = Engine::new(&config)?;
        engine.tick(Duration::from_secs_f32(tuning.tick / 1000.0))?;
        Ok(Self(engine))
    }
}

/// The WASM host's methods on [`Engine`].
trait WasmEngineExt {
    /// Ticks the engine's epoch every `every` until the engine is
    /// gone.
    fn tick(&self, every: Duration) -> std::io::Result<()>;
}

impl WasmEngineExt for Engine {
    fn tick(&self, every: Duration) -> std::io::Result<()> {
        let weak = self.weak();
        thread::Builder::new()
            .name("wasm epoch".into())
            .spawn(move || {
                while let Some(engine) = weak.upgrade() {
                    engine.increment_epoch();
                    drop(engine);
                    thread::sleep(every);
                }
            })
            .map(drop)
    }
}

fn start(
    tuning: Res<WasmTuning>,
    mut log: MessageWriter<LogLine>,
    mut commands: Commands,
) {
    match Host::new(&tuning) {
        Ok(host) => commands.insert_resource(host),
        Err(why) => {
            log.write(LogLine::error(
                "wasm",
                format!("no plugin can run: {why}"),
            ));
        }
    }
}
