//! What every part of the Hamlet window is built on, and all a part
//! may use: Bevy, fynix, the widget kit, the connection to the server
//! and the realm it holds, and the ways a part joins the window.
//!
//! The window's own parts reach the other Hamlet crates only through
//! this one, as a plugin does.

pub mod age;
pub mod clock;
pub mod panels;
pub mod roll;
pub mod standing;
pub mod words;

pub use bevy;
use bevy::app::App;
pub use bevy_fynix as fynix;
pub use hamlet_assets as assets;
pub use hamlet_client as client;
pub use hamlet_desktop as desktop;
pub use hamlet_protocol as protocol;
pub use hamlet_ui as ui;

/// What every part of the window needs before it is added.
pub fn plugin(app: &mut App) {
    app.add_plugins((panels::plugin, roll::plugin));
}
