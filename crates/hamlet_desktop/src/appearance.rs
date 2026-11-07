//! Whether the computer is light or dark.

use bevy_app::App;
use bevy_ecs::resource::Resource;
use bevy_window::WindowTheme;

pub(crate) fn plugin(app: &mut App) {
    // Nothing asks the computer yet, so the panels stay dark.
    app.insert_resource(SystemAppearance::Silent);
}

/// What the computer has said.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug)]
pub enum SystemAppearance {
    /// Asked, no answer yet.
    Asking,
    Says(WindowTheme),
    /// No answer in time, or none to be had.
    Silent,
}
