//! The engine: its modules, composed. `cargo run -p hamlet_engine`.

mod demo;

use bevy::DefaultPlugins;
use bevy::app::{App, AppExit, PluginGroup};
use hamlet_engine_core::Bundle;

fn main() -> AppExit {
    App::new()
        .insert_resource(Bundle::find(env!("CARGO_MANIFEST_DIR")))
        .add_plugins((
            DefaultPlugins.set(hamlet_engine_shell::window_plugin()),
            hamlet_engine_core::plugin,
            hamlet_engine_shell::plugin,
            hamlet_engine_plugins::plugin,
            hamlet_engine_grants::plugin,
            hamlet_engine_assets::plugin,
            hamlet_engine_bus::plugin,
            hamlet_engine_net::plugin,
            hamlet_engine_agents::plugin,
            hamlet_engine_wasm::plugin,
            demo::plugin,
        ))
        .run()
}
