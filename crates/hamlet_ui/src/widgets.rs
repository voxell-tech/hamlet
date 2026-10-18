//! What every panel is built from.

pub mod feel;
pub mod tuning;

use bevy::app::App;
pub use hamlet_sdk::{icons, platform, tabs};

pub fn plugin(app: &mut App) {
    app.add_plugins((
        tuning::plugin,
        platform::plugin,
        icons::plugin,
        feel::plugin,
    ));
}
