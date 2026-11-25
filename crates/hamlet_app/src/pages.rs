//! The window's own pages of Settings.

pub mod appearance;
pub mod sight;
pub mod sound;
pub mod updates;

use bevy::app::App;

pub fn plugin(app: &mut App) {
    app.add_plugins((
        appearance::plugin,
        sight::plugin,
        sound::plugin,
        updates::plugin,
    ));
}
