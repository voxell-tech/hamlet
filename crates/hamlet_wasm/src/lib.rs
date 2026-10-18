//! The host of the installed plugins: WASM components run under
//! limits, one store each.

pub mod instance;

use bevy::app::App;

/// Runs the installed plugins.
pub fn plugin(_app: &mut App) {}
