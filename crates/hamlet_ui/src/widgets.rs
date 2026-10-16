//! What every panel is built from.

pub mod feel;
pub mod fields;
pub mod icons;
pub mod platform;
pub mod tabs;
pub mod tuning;

use bevy::app::App;

pub fn plugin(app: &mut App) {
    app.add_plugins((
        tuning::plugin,
        tabs::plugin,
        platform::plugin,
        icons::plugin,
        feel::plugin,
        fields::plugin,
    ));
}
