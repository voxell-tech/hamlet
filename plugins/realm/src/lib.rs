//! The realm layer: the generated world, the ground, the sites, the
//! camera and the effects, drawn from the hamlets. Its view in the
//! window is [`view`].

pub mod view;

use bevy::app::App;
use hamlet_realm::{
    bubble, chief, debug, generator, hamlet, hand, keeper, lantern,
    library, moving, needs, orders, palette, pick, play, settings,
    sky, sound, stage, stock, survey, works,
};
use hamlet_sdk::plugin::{
    AppPluginExt, Entry, HamletPlugin, Manifest,
};

/// Adds the Realm plugin.
pub fn plugin(app: &mut App) {
    app.add_plugins(world).add_hamlet_plugin::<RealmLayer>();
}

/// The realm's modules: the generated world and everything that lives
/// in it. Add it after Bevy's default plugins. An app that wants only
/// the scene, with no hamlets, adds the first group itself.
pub fn world(app: &mut App) {
    app.add_plugins((
        // The scene: tuning, colours, camera, clock, robots and the
        // shared materials. Later plugins read them.
        (
            settings::plugin,
            palette::plugin,
            play::plugin,
            sound::plugin,
            debug::plugin,
            sky::plugin,
            stage::plugin,
            keeper::plugin,
            stock::plugin,
        ),
        // What is drawn in it.
        generator::plugin,
        // What the chief does in it.
        (
            pick::plugin,
            needs::plugin,
            bubble::plugin,
            hand::plugin,
            orders::plugin,
            chief::plugin,
            library::plugin,
            moving::plugin,
            works::plugin,
            lantern::plugin,
        ),
        // The core's entities, drawn, and what a plugin reads of
        // them.
        (hamlet::plugin, survey::plugin),
    ));
}

/// The plugin that draws the world.
pub struct RealmLayer;

impl HamletPlugin for RealmLayer {
    fn manifest() -> Manifest {
        Manifest::built_in("realm")
            .named("Realm")
            .needs("hamlets", ">=0.1, <1")
            .essential()
    }

    fn entry(_: &mut Entry) {}
}
