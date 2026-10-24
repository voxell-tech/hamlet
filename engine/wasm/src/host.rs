//! The wasmtime engine every plugin runs on, and the thread that
//! ticks its epoch.

use std::path::Path;
use std::time::Duration;
use std::{fs, thread};

use bevy::app::{App, Startup};
use bevy::ecs::message::MessageWriter;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Res};
use hamlet_engine_core::tuning::{ENGINE, Tuning, TuningFile};
use hamlet_engine_core::{LogLine, TuningPlugin};
use serde::Deserialize;
use wasmtime::{
    Config, Engine, InstanceAllocationStrategy, OptLevel,
};

pub(crate) fn plugin(app: &mut App) {
    app.add_plugins(TuningPlugin::<WasmTuning>::default())
        .add_systems(Startup, start);
}

/// How the host runs plugins. The tick is read as the engine starts.
#[derive(Resource, Clone, PartialEq, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WasmTuning {
    /// Milliseconds between two ticks of the epoch, which a call's
    /// time is counted in.
    pub tick: f32,
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
    /// Ticks of the epoch a call of `time` lasts, at least one.
    /// wasmtime adds them to the epoch, so they stay far from
    /// `u64::MAX`.
    pub(crate) fn ticks(&self, time: Duration) -> u64 {
        (time.as_secs_f64() * 1000.0 / f64::from(self.tick))
            .ceil()
            .clamp(1.0, f64::from(u32::MAX)) as u64
    }
}

/// The engine every plugin runs on.
#[derive(Resource, Clone)]
pub struct Host(pub(crate) Engine);

impl Host {
    /// An engine for components, its code compiled for speed, with an
    /// epoch ticked for as long as it lives.
    pub fn new(tuning: &WasmTuning) -> wasmtime::Result<Self> {
        let engine = Engine::new(&config())?;
        engine.tick(Duration::from_secs_f32(tuning.tick / 1000.0))?;
        Ok(Self(engine))
    }
}

/// How plugins are compiled and run. A component compiled by an
/// engine of another configuration is refused, so [`precompile`]
/// builds its engine from this too.
fn config() -> Config {
    let mut config = Config::new();
    // Each instance is given its memories and tables as it is made,
    // as large as it asks, with no pool to cap how many plugins run
    // or how much each holds. A memory still sits in a reservation
    // with guard pages, so its accesses need no bounds checks.
    config
        .wasm_component_model(true)
        .cranelift_opt_level(OptLevel::Speed)
        .epoch_interruption(true)
        .allocation_strategy(InstanceAllocationStrategy::OnDemand);
    config
}

/// Compiles the `plugin.wasm` at `wasm` for the engine and keeps the
/// result in `into`, so the plugin starts without compiling from the
/// folder that holds `into`. Run by `scripts/build_plugins.sh`.
pub fn precompile(wasm: &Path, into: &Path) -> Result<(), String> {
    let bytes = fs::read(wasm).map_err(|e| {
        format!("{} cannot be read: {e}", wasm.display())
    })?;
    let engine =
        Engine::new(&config()).map_err(|e| format!("{e:#}"))?;
    let compiled = engine
        .precompile_component(&bytes)
        .map_err(|e| format!("{e:#}"))?;
    crate::compiled::keep(into, &bytes, compiled)
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
