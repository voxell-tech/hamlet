//! A shortcut, built once: the chord a command is bound to, and the
//! keys as the keyboard prints them, `⌘⇧K` on a Mac and
//! `Ctrl+Shift+K` elsewhere.

use std::fmt;

use bevy::input::keyboard::KeyCode;
use bevy_fynix::ScopedExt;
use bevy_fynix::shortcut::{Chord, Mods};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::label;

use crate::View;
use crate::styles::small_rules;

/// A key and the modifiers held with it, built from the key out, as
/// `Keys::new(KeyCode::KeyK).primary().shift()`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Keys {
    key: KeyCode,
    mods: Mods,
}

impl Keys {
    /// `key` alone.
    pub const fn new(key: KeyCode) -> Self {
        Self {
            key,
            mods: Mods::NONE,
        }
    }

    /// With Command on macOS, Control elsewhere.
    pub const fn primary(self) -> Self {
        self.with(Mods::PRIMARY)
    }

    pub const fn shift(self) -> Self {
        self.with(Mods::SHIFT)
    }

    /// With Option on macOS, Alt elsewhere.
    pub const fn alt(self) -> Self {
        self.with(Mods::ALT)
    }

    /// With Control on macOS, where it is not the primary key.
    pub const fn ctrl(self) -> Self {
        self.with(Mods::CTRL)
    }

    const fn with(self, mods: Mods) -> Self {
        Self {
            mods: self.mods.with(mods),
            ..self
        }
    }

    /// The chord a command is bound to.
    pub const fn chord(self) -> Chord {
        Chord {
            key: self.key,
            mods: self.mods,
        }
    }

    /// The keys in small faint letters, as a menu or a button gives
    /// them beside what they do.
    pub fn view(self) -> impl View {
        label(self.to_string())
            .tone(Tone::Faint)
            .wrap(false)
            .rules(small_rules)
    }
}

impl From<Chord> for Keys {
    fn from(chord: Chord) -> Self {
        Self {
            key: chord.key,
            mods: chord.mods,
        }
    }
}

impl fmt::Display for Keys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mac = cfg!(target_os = "macos");
        let held = |bit: Mods| self.mods.has(bit);
        if mac {
            // In the order a Mac's menus print them.
            for (bit, mark) in [
                (Mods::CTRL, "⌃"),
                (Mods::ALT, "⌥"),
                (Mods::SHIFT, "⇧"),
                (Mods::PRIMARY, "⌘"),
            ] {
                if held(bit) {
                    f.write_str(mark)?;
                }
            }
            return f.write_str(&face(self.key, mac));
        }
        let meta = if cfg!(windows) { "Win" } else { "Super" };
        for (bit, name) in [
            (Mods::PRIMARY, "Ctrl"),
            (Mods::CTRL, "Ctrl"),
            (Mods::META, meta),
            (Mods::ALT, "Alt"),
            (Mods::SHIFT, "Shift"),
        ] {
            if held(bit) {
                write!(f, "{name}+")?;
            }
        }
        f.write_str(&face(self.key, mac))
    }
}

/// What is printed on `key`, where a Mac and other keyboards print
/// some keys differently.
fn face(key: KeyCode, mac: bool) -> String {
    let both = |on_mac: &str, elsewhere: &str| {
        if mac { on_mac } else { elsewhere }.to_string()
    };
    match key {
        KeyCode::Enter | KeyCode::NumpadEnter => both("↩", "Enter"),
        KeyCode::Escape => both("⎋", "Esc"),
        KeyCode::Tab => both("⇥", "Tab"),
        KeyCode::Backspace => both("⌫", "Backspace"),
        KeyCode::Delete => both("⌦", "Del"),
        KeyCode::Home => both("↖", "Home"),
        KeyCode::End => both("↘", "End"),
        KeyCode::PageUp => both("⇞", "PgUp"),
        KeyCode::PageDown => both("⇟", "PgDn"),
        KeyCode::Space => "Space".into(),
        KeyCode::ArrowLeft => "←".into(),
        KeyCode::ArrowRight => "→".into(),
        KeyCode::ArrowUp => "↑".into(),
        KeyCode::ArrowDown => "↓".into(),
        KeyCode::Comma => ",".into(),
        KeyCode::Period => ".".into(),
        KeyCode::Slash => "/".into(),
        KeyCode::Backslash => "\\".into(),
        KeyCode::Semicolon => ";".into(),
        KeyCode::Quote => "'".into(),
        KeyCode::Backquote => "`".into(),
        KeyCode::BracketLeft => "[".into(),
        KeyCode::BracketRight => "]".into(),
        KeyCode::Minus | KeyCode::NumpadSubtract => "-".into(),
        KeyCode::Equal => "=".into(),
        KeyCode::NumpadAdd => "+".into(),
        _ => {
            // `KeyW` and `Digit1` read as the key's own face, and
            // `F5` as itself.
            let name = format!("{key:?}");
            name.strip_prefix("Key")
                .or_else(|| name.strip_prefix("Digit"))
                .or_else(|| name.strip_prefix("Numpad"))
                .unwrap_or(&name)
                .to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_read_as_the_keyboard_prints_them() {
        let send = Keys::new(KeyCode::Enter).primary();
        let settings = Keys::new(KeyCode::Comma).primary();
        let back = Keys::new(KeyCode::Tab).ctrl().shift();
        if cfg!(target_os = "macos") {
            assert_eq!(send.to_string(), "⌘↩");
            assert_eq!(settings.to_string(), "⌘,");
            assert_eq!(back.to_string(), "⌃⇧⇥");
        } else {
            assert_eq!(send.to_string(), "Ctrl+Enter");
            assert_eq!(settings.to_string(), "Ctrl+,");
            assert_eq!(back.to_string(), "Ctrl+Shift+Tab");
        }
        assert_eq!(Keys::from(send.chord()), send);
    }
}
