//! What every panel is built from.

pub mod dial;
pub mod disclosure;
pub mod effects;
pub mod fade;
pub mod icons;
pub mod platform;
pub mod switch;
pub mod tabs;

use bevy::app::App;

pub fn plugin(app: &mut App) {
    app.add_plugins((
        tabs::plugin,
        platform::plugin,
        icons::plugin,
        effects::plugin,
        switch::plugin,
        fade::plugin,
    ));
}
