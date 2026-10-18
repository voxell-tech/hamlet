//! The Skills panel: an agent's own skills over the library every
//! agent shares.

mod library;
mod own;
mod row;
mod screen;

use bevy::app::App;
use hamlet_sdk::icons::Glyph;
use hamlet_sdk::menu::Menu;
use hamlet_sdk::plugin::{
    AppPluginExt, Entry, HamletPlugin, Manifest,
};

/// Adds the Skills plugin.
pub fn plugin(app: &mut App) {
    app.add_hamlet_plugin::<SkillsPlugin>();
}

/// The plugin that adds the Skills panel.
pub struct SkillsPlugin;

impl HamletPlugin for SkillsPlugin {
    fn manifest() -> Manifest {
        Manifest::built_in("skills").named("Skills")
    }

    fn entry(entry: &mut Entry) {
        // Closed until asked for from a tab bar's plus.
        entry.panel("skills", "SKILLS", Glyph::Book, screen::screen);
        entry.opens("open", "Skills", "skills");
        entry.menu_item(
            Menu::View,
            1,
            "Skills",
            "open",
            Some(Glyph::Book),
        );
    }
}
