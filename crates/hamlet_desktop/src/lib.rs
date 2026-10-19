//! The machine the app runs on: kept awake while an agent works
//! (`awake`), the programs of the system that show a file or a link
//! (`open`), its dialogs a folder or a file is chosen in (`folder`,
//! `file`) and the app's icon (`app_icon`).

pub mod app_icon;
pub mod awake;
pub mod file;
pub mod folder;
pub mod open;

use bevy::app::{App, Plugin};

/// The machine's part in every mode, as an agent works in each.
/// `window` adds what only a window uses: the app's icon and the
/// dialogs.
pub struct DesktopPlugin {
    pub window: bool,
}

impl Plugin for DesktopPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(awake::plugin);
        if self.window {
            app.add_plugins((app_icon::plugin, folder::plugin))
                .insert_resource(file::FileDialog(file::system));
        }
    }
}
