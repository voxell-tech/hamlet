//! The engine: its modules, composed. `cargo run -p hamlet_engine`.

use bevy::DefaultPlugins;
use bevy::app::{App, AppExit, PluginGroup};
use hamlet_engine_core::{Bundle, Home};

fn main() -> AppExit {
    let home = Home::from_env();
    let mut app = App::new();
    match hamlet_engine_desktop::claim(&home) {
        Ok(Some(launches)) => {
            app.insert_resource(launches);
        }
        Ok(None) => {
            eprintln!(
                "hamlet already runs on {}, and was asked to show \
                 its window",
                home.root().display()
            );
            return AppExit::Success;
        }
        Err(why) => eprintln!(
            "{} cannot be claimed, so a later launch will not show \
             this window: {why}",
            home.root().display()
        ),
    }
    app.insert_resource(home)
        .insert_resource(Bundle::find(env!("CARGO_MANIFEST_DIR")))
        .add_plugins((
            DefaultPlugins.set(hamlet_engine_shell::window_plugin()),
            hamlet_engine_core::plugin,
            hamlet_engine_audio::plugin,
            hamlet_engine_shell::plugin,
            hamlet_engine_plugins::plugin,
            hamlet_engine_grants::plugin,
            hamlet_engine_assets::plugin,
            hamlet_engine_bus::plugin,
            hamlet_engine_net::plugin,
            hamlet_engine_agents::plugin,
            hamlet_engine_desktop::plugin,
            hamlet_engine_render::plugin,
            hamlet_engine_wasm::plugin,
        ))
        .run()
}
