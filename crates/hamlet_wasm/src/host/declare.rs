//! What a plugin declares during `entry`.

use hamlet_sdk::icons::Glyph;

use super::State;
use crate::glyph;
use crate::interface::hamlet::plugin::host::Level;

/// A panel a plugin declared.
#[derive(Clone, PartialEq, Debug)]
pub struct Declared {
    /// The id the plugin knows the panel by.
    pub id: String,
    pub title: String,
    pub icon: Glyph,
}

impl State {
    pub(super) fn declare(
        &mut self,
        id: String,
        title: String,
        icon: String,
    ) {
        if self.declared.len() >= self.panel_room
            || self.declared.iter().any(|d| d.id == id)
        {
            return;
        }
        let named = glyph::named(&icon);
        if named.is_none() {
            self.write(
                Level::Warn,
                format!("no icon named `{icon}`"),
            );
        }
        self.declared.push(Declared {
            id,
            title,
            icon: named.unwrap_or(glyph::UNKNOWN),
        });
    }
}
