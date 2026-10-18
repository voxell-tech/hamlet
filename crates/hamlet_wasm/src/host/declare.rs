//! What a plugin declares during `entry`.

use hamlet_sdk::icons::Glyph;

use super::State;
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
        let glyph = Glyph::ALL
            .into_iter()
            .find(|g| format!("{g:?}").eq_ignore_ascii_case(&icon));
        if glyph.is_none() {
            self.write(
                Level::Warn,
                format!("no icon named `{icon}`"),
            );
        }
        self.declared.push(Declared {
            id,
            title,
            icon: glyph.unwrap_or(Glyph::Menu),
        });
    }
}
