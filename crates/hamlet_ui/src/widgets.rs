//! What every panel is built from.

pub mod diff;
pub mod feel;
pub mod icons;
pub mod platform;
pub mod tabs;
pub mod tuning;

use bevy::app::App;

pub fn plugin(app: &mut App) {
    app.add_plugins((
        diff::plugin,
        tuning::plugin,
        tabs::plugin,
        platform::plugin,
        icons::plugin,
        feel::plugin,
    ));
}
