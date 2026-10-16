//! The panels that draw from the core's state alone.

pub mod chat;
pub mod inspector;
pub mod links;
pub mod overview;
pub mod petition;
pub mod pictures;
pub mod settings;

use bevy::app::App;

pub fn plugin(app: &mut App) {
    app.add_plugins((
        inspector::plugin,
        overview::plugin,
        chat::plugin,
        links::plugin,
        pictures::plugin,
        petition::plugin,
        settings::plugin,
    ));
}
