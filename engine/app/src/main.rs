//! The engine: its modules, composed. `cargo run -p hamlet_engine`.

use bevy::DefaultPlugins;
use bevy::app::{App, AppExit, PluginGroup};

fn main() -> AppExit {
    App::new()
        .add_plugins((
            DefaultPlugins.set(hamlet_engine_shell::window_plugin()),
            hamlet_engine_core::plugin,
            hamlet_engine_shell::plugin,
        ))
        .run()
}
