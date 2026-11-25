//! The panels that draw from the client's state.

pub mod chat;
pub mod hub;
pub mod links;
pub mod overview;
pub mod pictures;
pub mod viewer;
pub mod wait;

use bevy::app::App;

pub fn plugin(app: &mut App) {
    app.add_plugins((
        hub::plugin,
        overview::plugin,
        chat::plugin,
        links::plugin,
        pictures::plugin,
        hamlet_terminal::plugin,
        viewer::plugin,
        wait::plugin,
    ));
}
