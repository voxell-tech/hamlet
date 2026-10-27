//! The machine the app runs on: kept awake while an agent works
//! (`awake`), the programs of the system that show a file or a link
//! (`open`), temporary documents watched for saves (`editor`), its
//! dialogs a folder (`folder`) or a file (`file`) is chosen in, the
//! app's icon (`app_icon`) and the window's drag by its title bar
//! (`title_bar`).

pub mod app_icon;
pub mod awake;
pub mod editor;
pub mod file;
pub mod folder;
pub mod open;
pub mod title_bar;

use bevy_app::{App, Plugin};

/// The machine's part in every mode, as an agent works in each.
/// `window` adds what only a window uses: the app's icon, the folder
/// and file dialogs and the title bar's drag.
pub struct DesktopPlugin {
    pub window: bool,
}

impl Plugin for DesktopPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((awake::plugin, editor::plugin));
        if self.window {
            app.add_plugins((
                app_icon::plugin,
                folder::plugin,
                file::plugin,
                title_bar::plugin,
            ));
        }
    }
}
