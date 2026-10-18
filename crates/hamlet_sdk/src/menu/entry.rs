//! A line a plugin adds to a menu of the top bar.

use bevy::ecs::component::Component;

use crate::icons::Glyph;

/// A menu of the top bar. On a title, the menu it opens.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Component)]
pub enum Menu {
    App,
    View,
}

impl Menu {
    /// The name a plugin gives it: `hamlet` or `view`, in any case.
    pub fn parse(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "hamlet" => Some(Menu::App),
            "view" => Some(Menu::View),
            _ => None,
        }
    }
}

/// On an entity a plugin declared: a line of a menu that runs one of
/// its commands.
#[derive(Component, Clone, PartialEq, Eq, Debug)]
#[component(immutable)]
pub struct MenuEntry {
    pub menu: Menu,
    /// The group it stands in. Groups run in this order with a line
    /// between them.
    pub section: u32,
    pub label: String,
    /// The name of a command of the same plugin.
    pub command: String,
    /// Falls to the command's icon where there is none.
    pub icon: Option<Glyph>,
}
