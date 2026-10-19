//! Where the chief changes the limits and the models: the home's
//! endpoints and catalogue, and each hamlet's budget, votes, models
//! and the mayor's leave.

pub mod models;
pub mod rules;

use bevy::app::App;
use bevy_fynix::ViewExt;
pub use hamlet_agents_api::roll;
pub use hamlet_chat_api::chat;
use hamlet_limits_api::Panes;
use hamlet_sdk::plugin::{
    AppPluginExt, Entry, HamletPlugin, Manifest,
};
pub use hamlet_sdk::{
    AnyView, Cx, View, icons, looks, platform, with_cx,
};

/// The panes other plugins show, as [`Panes`] holds them.
pub const PANES: Panes = Panes {
    endpoints: || models::endpoints().boxed(),
    shelf: models::shelf,
    of_agent: models::of_agent,
    realm: || rules::realm().boxed(),
    rules: rules::panel,
};

/// The panes of the limits. Add it after the SDK's plugin.
pub fn plugin(app: &mut App) {
    app.add_plugins((models::plugin, rules::plugin))
        .insert_resource(PANES)
        .add_hamlet_plugin::<LimitsPlugin>();
}

/// The built-in plugin of the panes of the limits. Essential: the
/// first-run screen and the Settings choose the models with them, so
/// a home with no model that answers is put right only through them.
pub struct LimitsPlugin;

impl HamletPlugin for LimitsPlugin {
    fn manifest() -> Manifest {
        Manifest::built_in("limits").named("Limits").essential()
    }

    fn entry(_: &mut Entry) {}
}
