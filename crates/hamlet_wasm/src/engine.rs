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
    /// `limits`, and its ticker running.
    pub fn new(limits: &Limits) -> wasmtime::Result<Self> {
        let mut config = Config::new();
        config.wasm_component_model(true).epoch_interruption(true);
        let mut pool = PoolingAllocationConfig::new();
        pool.max_memory_size(limits.memory_bytes())
            .table_elements(limits.table_elements);
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
