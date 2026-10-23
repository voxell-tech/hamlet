//! The status items plugins add. Nothing draws them: the window has
//! no status line, and what it showed is in the console.

use bevy::ecs::component::Component;
use bevy_fynix::tokens::Tone;
use hamlet_engine_api::PanelId;

use crate::icons::Glyph;

/// An item a plugin adds to the status: an icon and words in a tone,
/// and words for the pointer resting on it. One with no words is
/// hidden, unless it grows.
#[derive(Component, Clone, PartialEq, Debug)]
pub struct StatusItem {
    pub id: PanelId,
    pub icon: Option<Glyph>,
    pub text: String,
    pub tone: Tone,
    pub tip: String,
    /// It takes the room left on the line, and clips its words.
    pub grow: bool,
}

impl StatusItem {
    /// The item `id`, with nothing to show yet.
    pub fn new(id: PanelId, grow: bool) -> Self {
        Self {
            id,
            icon: None,
            text: String::new(),
            tone: Tone::Dim,
            tip: String::new(),
            grow,
        }
    }
}
