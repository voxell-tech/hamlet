//! What every panel is built from.

pub mod dial;
pub mod disclosure;
pub mod fade;
pub mod feel;
pub mod icons;
pub mod platform;
pub mod switch;
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
        switch::plugin,
        fade::plugin,
    ));
}
