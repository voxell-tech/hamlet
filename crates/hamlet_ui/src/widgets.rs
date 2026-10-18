//! What every panel is built from.

pub mod feel;
pub mod tabs;
pub mod tuning;

use bevy::app::App;
pub use hamlet_sdk::{icons, platform};

pub fn plugin(app: &mut App) {
    app.add_plugins((
        tuning::plugin,
        tabs::plugin,
        platform::plugin,
        icons::plugin,
        feel::plugin,
    ));
}
