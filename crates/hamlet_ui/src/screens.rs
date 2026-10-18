//! The panels that draw from the core's state alone.

pub mod inspector;
pub mod overview;
pub mod settings;

use bevy::app::App;

pub fn plugin(app: &mut App) {
    app.add_plugins((
        inspector::plugin,
        overview::plugin,
        settings::plugin,
    ));
}
