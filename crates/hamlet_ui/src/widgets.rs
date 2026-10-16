//! What every panel is built from.

pub mod feel;
pub use hamlet_sdk::{icons, platform};
pub mod tabs;

use bevy::app::App;

pub fn plugin(app: &mut App) {
    app.add_plugins((
        tabs::plugin,
        platform::plugin,
        icons::plugin,
        feel::plugin,
    ));
}
