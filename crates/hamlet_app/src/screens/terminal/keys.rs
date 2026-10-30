//! The bytes a terminal reads for a key, as xterm sends them.

use bevy::input::ButtonInput;
use bevy::input::keyboard::{Key, KeyCode};

/// The modifiers held with a key.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Mods {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    /// Command on macOS, Super elsewhere.
    pub command: bool,
}

impl Mods {
    pub fn held(keys: &ButtonInput<KeyCode>) -> Self {
        let any = |pair: [KeyCode; 2]| keys.any_pressed(pair);
        Self {
            shift: any([KeyCode::ShiftLeft, KeyCode::ShiftRight]),
            ctrl: any([KeyCode::ControlLeft, KeyCode::ControlRight]),
            alt: any([KeyCode::AltLeft, KeyCode::AltRight]),
            command: any([KeyCode::SuperLeft, KeyCode::SuperRight]),
        }
    }

    /// The modifier number of xterm's sequences: 1 for none.
    fn number(self) -> u8 {
        1 + u8::from(self.shift)
            + 2 * u8::from(self.alt)
            + 4 * u8::from(self.ctrl)
    }
}

/// What a terminal is told of its keys.
#[derive(Clone, Copy, Default, Debug)]
pub struct Modes {
    /// The arrows send their application forms.
    pub application_cursor: bool,
    /// Alt sends Escape before a character. Off, the character Alt
    /// makes is sent, as on macOS.
    pub alt_is_meta: bool,
}

/// The bytes for `key` held with `mods`, given the `text` it typed.
/// None for a key the terminal is not sent, such as a Command chord.
pub fn encode(
    key: &Key,
    text: Option<&str>,
    mods: Mods,
    modes: Modes,
) -> Option<String> {
    if mods.command {
        return None;
    }
    let escape = |rest: &str| {
        if mods.alt {
            format!("\x1b{rest}")
        } else {
            rest.to_owned()
        }
    };
    let number = mods.number();
    let cursor = |end: char| {
        Some(if number > 1 {
            format!("\x1b[1;{number}{end}")
        } else if modes.application_cursor {
            format!("\x1bO{end}")
        } else {
            format!("\x1b[{end}")
        })
    };
    let tilde = |code: u8| {
        Some(if number > 1 {
            format!("\x1b[{code};{number}~")
        } else {
            format!("\x1b[{code}~")
        })
    };
    let function = |end: char| {
        Some(if number > 1 {
            format!("\x1b[1;{number}{end}")
        } else {
            format!("\x1bO{end}")
        })
    };
    match key {
        Key::Enter => Some(escape("\r")),
        Key::Backspace if mods.ctrl => Some(escape("\x08")),
        Key::Backspace => Some(escape("\x7f")),
        Key::Tab if mods.shift => Some("\x1b[Z".into()),
        Key::Tab => Some(escape("\t")),
        Key::Escape => Some("\x1b".into()),
        Key::Space if mods.ctrl => Some("\0".into()),
        Key::Space => Some(escape(" ")),
        Key::ArrowUp => cursor('A'),
        Key::ArrowDown => cursor('B'),
        Key::ArrowRight => cursor('C'),
        Key::ArrowLeft => cursor('D'),
        Key::Home => cursor('H'),
        Key::End => cursor('F'),
        Key::Insert => tilde(2),
        Key::Delete => tilde(3),
        Key::PageUp => tilde(5),
        Key::PageDown => tilde(6),
        Key::F1 => function('P'),
        Key::F2 => function('Q'),
        Key::F3 => function('R'),
        Key::F4 => function('S'),
        Key::F5 => tilde(15),
        Key::F6 => tilde(17),
        Key::F7 => tilde(18),
        Key::F8 => tilde(19),
        Key::F9 => tilde(20),
        Key::F10 => tilde(21),
        Key::F11 => tilde(23),
        Key::F12 => tilde(24),
        Key::Character(typed) if mods.ctrl => {
            let mut chars = typed.chars();
            let (Some(c), None) = (chars.next(), chars.next()) else {
                return None;
            };
            control(c)
                .map(|byte| escape(&char::from(byte).to_string()))
        }
        Key::Character(typed) if mods.alt && modes.alt_is_meta => {
            Some(format!("\x1b{typed}"))
        }
        _ => text
            .filter(|text| !text.chars().any(char::is_control))
            .map(str::to_owned),
    }
}

/// The control byte Ctrl makes of `c`.
fn control(c: char) -> Option<u8> {
    Some(match c.to_ascii_lowercase() {
        c @ 'a'..='z' => c as u8 - b'a' + 1,
        '@' | ' ' | '2' => 0,
        '[' | '3' => 0x1b,
        '\\' | '4' => 0x1c,
        ']' | '5' => 0x1d,
        '^' | '6' => 0x1e,
        '_' | '-' | '/' | '7' => 0x1f,
        '?' | '8' => 0x7f,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const CTRL: Mods = Mods {
        shift: false,
        ctrl: true,
        alt: false,
        command: false,
    };

    fn ch(c: &str) -> Key {
        Key::Character(c.into())
    }

    fn plain(key: &Key, text: Option<&str>) -> Option<String> {
        encode(key, text, Mods::default(), Modes::default())
    }

    #[test]
    fn the_arrows_take_their_application_form_and_xterms_modifiers() {
        let app = Modes {
            application_cursor: true,
            ..Modes::default()
        };
        let none = Mods::default();
        assert_eq!(plain(&Key::ArrowUp, None).unwrap(), "\x1b[A");
        assert_eq!(
            encode(&Key::ArrowUp, None, none, app).unwrap(),
            "\x1bOA"
        );
        assert_eq!(
            encode(&Key::ArrowLeft, None, CTRL, app).unwrap(),
            "\x1b[1;5D"
        );
        let shift_alt = Mods {
            shift: true,
            alt: true,
            ..none
        };
        assert_eq!(
            encode(&Key::Delete, None, shift_alt, app).unwrap(),
            "\x1b[3;4~"
        );
        assert_eq!(
            encode(&Key::F5, None, CTRL, app).unwrap(),
            "\x1b[15;5~"
        );
    }

    #[test]
    fn ctrl_makes_control_bytes_of_letters_and_brackets() {
        let key = |c| encode(&ch(c), None, CTRL, Modes::default());
        assert_eq!(key("c").unwrap(), "\x03");
        assert_eq!(key("C").unwrap(), "\x03");
        assert_eq!(key("[").unwrap(), "\x1b");
        assert_eq!(key("]").unwrap(), "\x1d");
        assert_eq!(key("\\").unwrap(), "\x1c");
        assert_eq!(key("ab"), None);
    }

    #[test]
    fn alt_sends_escape_first_only_where_it_is_meta() {
        let alt = Mods {
            alt: true,
            ..Mods::default()
        };
        let meta = Modes {
            alt_is_meta: true,
            ..Modes::default()
        };
        assert_eq!(
            encode(&ch("b"), Some("b"), alt, meta).unwrap(),
            "\x1bb"
        );
        // On macOS, Option and b type the character they make.
        assert_eq!(
            encode(&ch("b"), Some("\u{222b}"), alt, Modes::default())
                .unwrap(),
            "\u{222b}"
        );
    }

    #[test]
    fn typed_text_goes_as_it_is_and_command_chords_not_at_all() {
        assert_eq!(plain(&ch("a"), Some("A")).unwrap(), "A");
        assert_eq!(plain(&Key::Enter, Some("\r")).unwrap(), "\r");
        let shift = Mods {
            shift: true,
            ..Mods::default()
        };
        assert_eq!(
            encode(&Key::Tab, None, shift, Modes::default()).unwrap(),
            "\x1b[Z"
        );
        let command = Mods {
            command: true,
            ..Mods::default()
        };
        assert_eq!(
            encode(&ch("v"), Some("v"), command, Modes::default()),
            None
        );
    }
}
