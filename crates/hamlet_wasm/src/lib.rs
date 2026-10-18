//! The host of the installed plugins: WASM components run under
//! limits, one store each.

pub mod engine;
pub mod install;
pub mod instance;
pub mod interface;
pub mod limits;
pub mod manifest;

use bevy::prelude::*;
use hamlet_assets::TuningPlugin;

use self::engine::Host;
use self::limits::{Limits, WasmTuning};

/// Runs the installed plugins.
pub fn plugin(app: &mut App) {
    app.add_plugins(TuningPlugin::<WasmTuning>::default())
        .init_resource::<Limits>();
    match Host::new(&Limits::default()) {
        Ok(host) => {
            app.insert_resource(host);
        }
        Err(e) => error!("wasm: no engine: {e}"),
    }
}
