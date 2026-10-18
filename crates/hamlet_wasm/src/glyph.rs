//! The icons of the panels, by the names a plugin gives them.

use hamlet_sdk::icons::Glyph;

/// The icon of this name, in any case.
pub fn named(name: &str) -> Option<Glyph> {
    Glyph::ALL
        .into_iter()
        .find(|g| format!("{g:?}").eq_ignore_ascii_case(name))
}

/// The icon a plugin gets for a name that is not one.
pub const UNKNOWN: Glyph = Glyph::Menu;
