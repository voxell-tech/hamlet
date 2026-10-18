//! A command: the one action that menus, keys and plugins point at.

use bevy::ecs::component::Component;

use crate::icons::Glyph;

/// On an entity a plugin declared: an action with a name. The plugin
/// that owns it is the one it is
/// [`ContributedBy`](crate::plugin::ContributedBy).
#[derive(Component, Clone, PartialEq, Eq, Debug)]
#[component(immutable)]
pub struct Command {
    /// The name the owner exports it by.
    pub id: String,
    /// What a menu or a key shows for it.
    pub title: String,
    pub icon: Option<Glyph>,
}

impl Command {
    pub fn new(id: &str, title: &str) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            icon: None,
        }
    }

    pub fn icon(mut self, icon: Option<Glyph>) -> Self {
        self.icon = icon;
        self
    }
}

/// The id a [`RunCommand`](super::RunCommand) takes for the command
/// `command` of the plugin `plugin`.
pub fn full(plugin: &str, command: &str) -> String {
    format!("{plugin}/{command}")
}
