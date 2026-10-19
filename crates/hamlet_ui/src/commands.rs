//! The window's own commands and the lines they have in the menus.

use bevy::app::App;
use hamlet_sdk::icons::Glyph;
use hamlet_sdk::menu::Menu;
use hamlet_sdk::plugin::{
    AppPluginExt, Entry, HamletPlugin, Manifest,
};

use crate::window;

/// The section of the lines a menu always has.
const MAIN: u32 = 0;

/// The section that ends a menu, below what plugins add.
const LAST: u32 = 100;

/// Adds the plugin of the window's commands.
pub fn plugin(app: &mut App) {
    app.add_hamlet_plugin::<Window>();
}

/// The built-in plugin that owns the window's commands. Essential:
/// Quit is always there.
pub struct Window;

impl HamletPlugin for Window {
    fn manifest() -> Manifest {
        Manifest::built_in("window").named("Window").essential()
    }

    fn entry(entry: &mut Entry) {
        entry.command(
            "background",
            "Run in Background",
            |commands| {
                commands.queue(window::background);
            },
        );
        entry.command("quit", "Quit Hamlet", |commands| {
            commands.queue(window::quit);
        });
        entry.menu_item(
            Menu::App,
            MAIN,
            "Run in Background",
            "background",
            Some(Glyph::Lower),
        );
        entry.menu_item(
            Menu::App,
            LAST,
            "Quit Hamlet",
            "quit",
            Some(Glyph::Power),
        );
    }
}
