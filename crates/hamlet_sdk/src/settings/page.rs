//! A page a plugin adds to the settings.

use bevy::ecs::component::Component;

use crate::icons::Glyph;

/// On an entity a plugin declared: a door of the Settings panel to a
/// page of its own, which is a plugin panel.
#[derive(Component, Clone, PartialEq, Eq, Debug)]
#[component(immutable)]
pub struct SettingsPage {
    /// What the door and the head of the page say.
    pub title: String,
    pub icon: Glyph,
    /// What the door says under its title. It may be empty.
    pub holds: String,
    /// The id in the dock of the panel that fills the page.
    pub panel: String,
}
