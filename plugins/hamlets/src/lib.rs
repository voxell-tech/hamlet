//! The hamlets layer: what waits for a chief, counted, and the
//! petitions of plugins put among it, over the agents.

use bevy::app::App;
use hamlet_agents_api::{petition, waits};
use hamlet_sdk::plugin::{
    AppPluginExt, Entry, HamletPlugin, Manifest,
};

/// Adds the Hamlets plugin.
pub fn plugin(app: &mut App) {
    app.add_plugins((waits::plugin, petition::plugin))
        .add_hamlet_plugin::<HamletsPlugin>();
}

/// The plugin that holds what waits for a chief.
pub struct HamletsPlugin;

impl HamletPlugin for HamletsPlugin {
    fn manifest() -> Manifest {
        Manifest::built_in("hamlets")
            .named("Hamlets")
            .needs("agents", ">=0.1, <1")
            .essential()
    }

    fn entry(_: &mut Entry) {}
}
