//! Where the chief changes the limits and the models: the home's
//! endpoints and catalogue, and each hamlet's budget, votes, models
//! and the mayor's leave.

pub mod models;
pub mod rules;

use bevy::app::App;
pub use hamlet_agents_api::roll;
pub use hamlet_chat::chat;
pub use hamlet_sdk::{
    AnyView, Cx, View, icons, looks, platform, with_cx,
};

/// The panes of the limits. Add it after the SDK's plugin.
pub fn plugin(app: &mut App) {
    app.add_plugins((models::plugin, rules::plugin));
}
