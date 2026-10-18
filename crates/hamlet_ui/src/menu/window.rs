//! The window's own commands and the lines they have in the menus.

use bevy::app::App;
use hamlet_realm::pick::Orbit;
use hamlet_sdk::icons::Glyph;
use hamlet_sdk::menu::Menu;
use hamlet_sdk::plugin::{
    AppPluginExt, Entry, HamletPlugin, Manifest,
};

use crate::screens::settings;
use crate::world_view::TimeOfDay;
use crate::{window, world_menu};

/// The section of the lines a menu always has.
const MAIN: u32 = 0;

/// The section that ends a menu, below what plugins add.
const LAST: u32 = 100;

/// Adds the plugin of the window's commands.
pub fn plugin(app: &mut App) {
    app.add_hamlet_plugin::<Window>();
}

/// The built-in plugin that owns the window's commands.
pub struct Window;

impl HamletPlugin for Window {
    fn manifest() -> Manifest {
        Manifest::built_in("window").named("Window")
    }

    fn entry(entry: &mut Entry) {
        world_menu::contribute(entry);
        settings::contribute(entry);
        entry.command("found", "Found a Hamlet...", |commands| {
            commands.queue(world_menu::ask_found);
        });
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
        entry.command("reset-camera", "Reset Camera", |commands| {
            commands.insert_resource(Orbit::default());
        });
        entry.command(
            "sky-clock",
            "Sky Follows the Clock",
            |commands| {
                commands.insert_resource(TimeOfDay::Clock);
            },
        );
        entry.command("sky-day", "Sky at Day", |commands| {
            commands.insert_resource(TimeOfDay::Day);
        });
        entry.command("sky-night", "Sky at Night", |commands| {
            commands.insert_resource(TimeOfDay::Night);
        });
        for (menu, section, label, command, glyph) in [
            (
                Menu::App,
                MAIN,
                "Found a Hamlet...",
                "found",
                Glyph::Plus,
            ),
            (
                Menu::App,
                MAIN,
                "Run in Background",
                "background",
                Glyph::Lower,
            ),
            (Menu::App, LAST, "Quit Hamlet", "quit", Glyph::Power),
            (
                Menu::View,
                MAIN,
                "Reset Camera",
                "reset-camera",
                Glyph::Aim,
            ),
            (
                Menu::View,
                MAIN,
                "Sky Follows the Clock",
                "sky-clock",
                Glyph::Clock,
            ),
            (Menu::View, MAIN, "Sky at Day", "sky-day", Glyph::Sun),
            (
                Menu::View,
                MAIN,
                "Sky at Night",
                "sky-night",
                Glyph::Moon,
            ),
        ] {
            entry.menu_item(
                menu,
                section,
                label,
                command,
                Some(glyph),
            );
        }
    }
}
