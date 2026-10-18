//! Adding a plugin to the app.

use bevy::app::App;

use super::{EntryPoint, HamletPlugin, PluginInfo, Status};

/// Adds a [`HamletPlugin`]. [`crate::SdkPlugin`] is added first.
pub trait AppPluginExt {
    fn add_hamlet_plugin<P: HamletPlugin>(&mut self) -> &mut Self;
}

impl AppPluginExt for App {
    fn add_hamlet_plugin<P: HamletPlugin>(&mut self) -> &mut Self {
        let manifest = P::manifest();
        let point = EntryPoint(P::entry);
        let world = self.world_mut();
        let plugin = world
            .spawn((
                PluginInfo::from(&manifest),
                manifest,
                Status::Starting,
                point,
            ))
            .id();
        point.run(plugin, &mut world.commands());
        world.flush();
        self
    }
}
