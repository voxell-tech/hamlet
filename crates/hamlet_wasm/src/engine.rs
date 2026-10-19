//! The one engine every plugin runs on, and the ticker that advances
//! its epoch.

use std::thread;

use bevy::prelude::*;
use wasmtime::{
    Config, Engine, InstanceAllocationStrategy,
    PoolingAllocationConfig,
};

use crate::limits::Limits;

/// The engine, shared by every plugin.
#[derive(Resource, Clone)]
pub struct Host(pub Engine);

impl Host {
    /// An engine for components, with the pooling allocator sized to
    /// `limits`, and its ticker running. Each plugin's share of the
    /// pool is capped, so no plugin can take the room of another.
    pub fn new(limits: &Limits) -> wasmtime::Result<Self> {
        let mut config = Config::new();
        config
            .wasm_component_model(true)
            .epoch_interruption(true)
            .consume_fuel(true);
        // Room for a version being built beside its running one.
        let plugins = limits.pool_plugins + 1;
        let mut pool = PoolingAllocationConfig::new();
        pool.max_memory_size(limits.memory_bytes())
            .table_elements(limits.table_elements)
            .max_core_instances_per_component(limits.plugin_instances)
            .max_memories_per_component(limits.plugin_memories)
            .max_tables_per_component(limits.plugin_tables)
            .total_component_instances(plugins)
            .total_core_instances(plugins * limits.plugin_instances)
            .total_memories(plugins * limits.plugin_memories)
            .total_tables(plugins * limits.plugin_tables);
        config.allocation_strategy(
            InstanceAllocationStrategy::Pooling(pool),
        );
        let engine = Engine::new(&config)?;
        tick(&engine, limits);
        Ok(Self(engine))
    }
}

/// Advances the epoch of `engine` every tick until the engine is
/// gone.
fn tick(engine: &Engine, limits: &Limits) {
    let weak = engine.weak();
    let every = limits.tick();
    let ticker = thread::Builder::new()
        .name("wasm-epoch".into())
        .spawn(move || {
            while let Some(engine) = weak.upgrade() {
                engine.increment_epoch();
                drop(engine);
                thread::sleep(every);
            }
        });
    if let Err(e) = ticker {
        warn!("wasm: no epoch ticker: {e}");
    }
}
