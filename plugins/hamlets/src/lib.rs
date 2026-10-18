//! The hamlets layer: what waits for a chief, and the chief's
//! answers, over the agents.

use bevy::app::App;
use hamlet_core::oversight::OversightPlugin;
use hamlet_sdk::plugin::{
    AppPluginExt, Entry, HamletPlugin, Manifest,
};

/// Adds the Hamlets plugin.
pub fn plugin(app: &mut App) {
    app.add_plugins(OversightPlugin)
        .add_hamlet_plugin::<HamletsPlugin>();
}

/// The plugin that holds what waits for a chief.
pub struct HamletsPlugin;

impl HamletPlugin for HamletsPlugin {
    fn manifest() -> Manifest {
        Manifest::built_in("hamlets").named("Hamlets").essential()
    }

    fn entry(_: &mut Entry) {}
}
