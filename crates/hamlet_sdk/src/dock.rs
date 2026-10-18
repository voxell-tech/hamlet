//! The dock as a plugin sees it: its panels, what fills one and
//! bringing one forward.

mod fill;
mod panel;
mod tab;

use bevy::app::App;
use bevy_fynix::dock::DockRegistry;

pub use self::fill::{lit_panel, panel};
pub use self::panel::{Body, Panel, PanelAppExt};
pub use self::tab::{TabShown, show_tab};
use crate::theme::HamletTheme;

/// Keeps the dock's registry of windows in step with the [`Panel`]
/// entities there are.
pub fn plugin(app: &mut App) {
    app.init_resource::<DockRegistry<HamletTheme>>()
        .add_observer(panel::register)
        .add_observer(panel::unregister);
}
