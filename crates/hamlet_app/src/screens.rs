//! The panels that draw from the client's state.

pub mod chat;
pub mod hub;
pub mod links;
pub mod overview;
pub mod pictures;
pub mod settings;
pub mod terminal;
pub mod viewer;
pub mod wait;
pub mod web;

use bevy::app::App;

pub fn plugin(app: &mut App) {
    app.add_plugins((
        hub::plugin,
        overview::plugin,
        chat::plugin,
        links::plugin,
        pictures::plugin,
        settings::plugin,
        terminal::plugin,
        viewer::plugin,
        wait::plugin,
        web::plugin,
    ));
}
