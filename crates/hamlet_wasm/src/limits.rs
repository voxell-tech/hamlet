//! What a plugin may spend: time per call, memory and tables. The
//! values are in `assets/wasm.toml`.

use std::fmt;
use std::time::Duration;

use bevy::prelude::*;
use hamlet_assets::{File, Tuning, designed, follow};
use wasmtime::ResourceLimiter;

/// The limits no grant changes.
#[derive(Resource, Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    /// The time one call may take, in milliseconds.
    pub call_ms: u64,
    /// The interval between two ticks of the engine's epoch. A
    /// call's time is counted in these.
    pub tick_ms: u64,
    /// The memory one plugin may grow to, in MiB.
    pub memory_mib: usize,
    /// The elements one plugin's tables may hold.
    pub table_elements: usize,
    /// The log lines one call may leave. The rest are dropped.
    pub log_lines: usize,
    /// The bytes of one log line. The rest of a longer line is cut.
    pub log_bytes: usize,
    /// The panels one plugin may declare. The rest are dropped.
    pub panels: usize,
    /// The nodes a panel's tree may hold.
    pub tree_nodes: usize,
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
}

impl Limits {
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
}

impl Tuning for WasmTuning {
    const FILE: File = hamlet_assets::WASM;

    fn apply(&self, was: &Self, world: &mut World) {
        follow(world, &was.limits, &self.limits);
    }
}

designed!(WasmTuning { Limits => limits });

/// Why a plugin's memory or table could not grow.
#[derive(Debug)]
pub struct OverLimit(pub &'static str);

impl fmt::Display for OverLimit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the plugin went over its {} limit", self.0)
    }
}

impl std::error::Error for OverLimit {}

/// Holds one plugin's store to its [`Limits`]. A growth past a limit
/// traps the call.
pub struct Limiter {
    memory: usize,
    table_elements: usize,
}

impl Limiter {
    pub fn new(limits: &Limits) -> Self {
        Self {
            memory: limits.memory_bytes(),
            table_elements: limits.table_elements,
        }
    }
}

impl ResourceLimiter for Limiter {
    fn memory_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        if desired > self.memory {
            return Err(wasmtime::Error::new(OverLimit("memory")));
        }
        Ok(true)
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
}
