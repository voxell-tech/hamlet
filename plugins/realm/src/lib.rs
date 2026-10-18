//! The realm layer: the generated world, the ground, the sites, the
//! camera and the effects, drawn from the hamlets.

use bevy::app::App;
use hamlet_realm::RealmPlugin;
use hamlet_sdk::plugin::{
    AppPluginExt, Entry, HamletPlugin, Manifest,
};

/// Adds the Realm plugin.
pub fn plugin(app: &mut App) {
    app.add_plugins(RealmPlugin)
        .add_hamlet_plugin::<RealmLayer>();
}

/// The plugin that draws the world.
pub struct RealmLayer;

impl HamletPlugin for RealmLayer {
    fn manifest() -> Manifest {
        Manifest::built_in("realm").named("Realm").essential()
    }

    fn entry(_: &mut Entry) {}
}
