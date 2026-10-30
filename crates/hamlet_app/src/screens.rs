//! The panels that draw from the client's state.

pub mod chat;
pub mod inspector;
pub mod links;
pub mod overview;
pub mod petition;
pub mod pictures;
pub mod settings;
pub mod terminal;

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
        terminal::plugin,
    ));
}
