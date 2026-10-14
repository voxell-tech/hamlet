//! The machine the app runs on: kept awake while an agent works
//! (`awake`), the programs of the system that show a file or a link
//! (`open`), its dialog a folder is chosen in (`folder`) and the
//! app's icon (`app_icon`).

pub mod app_icon;
pub mod awake;
pub mod folder;
pub mod open;

use bevy::app::{App, Plugin};

/// The machine's part in every mode, as an agent works in each.
/// `window` adds what only a window uses: the app's icon and the
/// folder dialog.
pub struct DesktopPlugin {
    pub window: bool,
}

impl Plugin for DesktopPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(awake::plugin);
        if self.window {
            app.add_plugins((app_icon::plugin, folder::plugin));
        }
    }
}
