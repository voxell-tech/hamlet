//! Chords as text: `Cmd+Shift+N`, `Ctrl+K`, `F5`.

use bevy::input::keyboard::KeyCode;
use bevy_fynix::shortcut::{Chord, Mods};

const MAC: bool = cfg!(target_os = "macos");

const LETTERS: [KeyCode; 26] = [
    KeyCode::KeyA,
    KeyCode::KeyB,
    KeyCode::KeyC,
    KeyCode::KeyD,
    KeyCode::KeyE,
    KeyCode::KeyF,
    KeyCode::KeyG,
    KeyCode::KeyH,
    KeyCode::KeyI,
    KeyCode::KeyJ,
    KeyCode::KeyK,
    KeyCode::KeyL,
    KeyCode::KeyM,
    KeyCode::KeyN,
    KeyCode::KeyO,
    KeyCode::KeyP,
    KeyCode::KeyQ,
    KeyCode::KeyR,
    KeyCode::KeyS,
    KeyCode::KeyT,
    KeyCode::KeyU,
    KeyCode::KeyV,
    KeyCode::KeyW,
    KeyCode::KeyX,
    KeyCode::KeyY,
    KeyCode::KeyZ,
];

const DIGITS: [KeyCode; 10] = [
    KeyCode::Digit0,
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
    KeyCode::Digit7,
    KeyCode::Digit8,
    KeyCode::Digit9,
];

/// The keys that are not a letter or a digit, by the name a chord
/// shows them with.
const NAMED: [(&str, KeyCode); 29] = [
    ("F1", KeyCode::F1),
    ("F2", KeyCode::F2),
    ("F3", KeyCode::F3),
    ("F4", KeyCode::F4),
    ("F5", KeyCode::F5),
    ("F6", KeyCode::F6),
    ("F7", KeyCode::F7),
    ("F8", KeyCode::F8),
    ("F9", KeyCode::F9),
    ("F10", KeyCode::F10),
    ("F11", KeyCode::F11),
    ("F12", KeyCode::F12),
    ("Space", KeyCode::Space),
    ("Tab", KeyCode::Tab),
    ("Backspace", KeyCode::Backspace),
    ("Delete", KeyCode::Delete),
    ("Home", KeyCode::Home),
    ("End", KeyCode::End),
    ("PageUp", KeyCode::PageUp),
    ("PageDown", KeyCode::PageDown),
    ("ArrowUp", KeyCode::ArrowUp),
    ("ArrowDown", KeyCode::ArrowDown),
    ("ArrowLeft", KeyCode::ArrowLeft),
    ("ArrowRight", KeyCode::ArrowRight),
    ("Comma", KeyCode::Comma),
    ("Period", KeyCode::Period),
    ("Slash", KeyCode::Slash),
    ("Minus", KeyCode::Minus),
    ("Equal", KeyCode::Equal),
];

/// The chord `text` names, as `Cmd+Shift+N` does. `Cmd` is the
/// command key of a Mac and Control elsewhere, so a chord written
/// with it is the same shortcut on every platform. `Ctrl` is
/// Control, which off a Mac is the same key as `Cmd`. Escape and
/// Enter are not shortcut keys.
pub fn parse(text: &str) -> Result<Chord, String> {
    let mut parts =
        text.split('+').map(str::trim).collect::<Vec<_>>();
    let key = parts.pop().filter(|key| !key.is_empty());
    let Some(key) = key else {
        return Err(format!("`{text}` names no key"));
    };
    let mut mods = Mods::NONE;
    for part in parts {
        mods = mods.with(modifier(part)?);
    }
    Ok(Chord {
        key: key_named(key)?,
        mods,
    })
}

fn modifier(name: &str) -> Result<Mods, String> {
    Ok(match name.to_ascii_lowercase().as_str() {
        "cmd" | "command" => Mods::PRIMARY,
        "ctrl" | "control" if MAC => Mods::CTRL,
        "ctrl" | "control" => Mods::PRIMARY,
        "alt" | "option" => Mods::ALT,
        "shift" => Mods::SHIFT,
        "super" | "win" | "meta" if MAC => Mods::PRIMARY,
        "super" | "win" | "meta" => Mods::META,
        _ => return Err(format!("`{name}` is not a modifier")),
    })
}

fn key_named(name: &str) -> Result<KeyCode, String> {
    let lower = name.to_ascii_lowercase();
    if matches!(lower.as_str(), "escape" | "esc" | "enter" | "return")
    {
        return Err(format!("`{name}` is not a shortcut key"));
    }
    let mut chars = name.chars();
    if let (Some(one), None) = (chars.next(), chars.next()) {
        if let Some(at) = one
            .is_ascii_alphabetic()
            .then(|| (one.to_ascii_lowercase() as u8 - b'a') as usize)
        {
            return Ok(LETTERS[at]);
        }
        if let Some(digit) = one.to_digit(10) {
            return Ok(DIGITS[digit as usize]);
        }
    }
    NAMED
        .iter()
        .find(|(face, _)| face.eq_ignore_ascii_case(name))
        .map(|(_, key)| *key)
        .ok_or_else(|| format!("`{name}` is not a key"))
}
