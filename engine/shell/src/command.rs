//! Commands and the keys bound to them. A command is an entity with
//! a [`Command`]; whoever owns it observes [`RunCommand`] on it. Menu
//! entries and shortcuts point at a command by its id.

use bevy::app::App;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::event::EntityEvent;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::system::{Commands, EntityCommands, Query, Res};
use bevy::input::keyboard::{KeyCode, KeyboardInput};
use bevy::input::{ButtonInput, ButtonState};
use bevy::input_focus::{FocusedInput, InputFocus};
use bevy::text::EditableText;
use bevy::window::Window;
use bevy_fynix::shortcut::{Chord, Mods};
use hamlet_engine_api::CommandId;
use hamlet_engine_api::id::ENGINE;

use crate::keymap::Bindings;
use crate::shortcuts::Capturing;

pub(crate) fn plugin(app: &mut App) {
    app.add_observer(press);
}

/// Something the person can run, from a menu or by a key.
#[derive(Component, Clone, Debug)]
#[component(immutable)]
pub struct Command {
    pub id: CommandId,
    /// What a menu calls it.
    pub label: String,
}

/// Runs the command it is triggered on.
#[derive(EntityEvent, Clone, Copy, Debug)]
pub struct RunCommand {
    pub entity: Entity,
}

/// A key a command comes bound to, unless the person bound it to
/// others.
#[derive(Component, Clone, Debug)]
#[component(immutable)]
pub struct Shortcut {
    pub chord: Chord,
    pub command: CommandId,
}

/// A command's entry in one of the bar's menus.
#[derive(Component, Clone, Debug)]
#[component(immutable)]
pub struct MenuEntry {
    /// The menu's title.
    pub menu: String,
    pub command: CommandId,
}

/// Control: the primary key off macOS, where Command is.
pub const CTRL: Mods = if cfg!(target_os = "macos") {
    Mods::CTRL
} else {
    Mods::PRIMARY
};

/// The chord `keys` writes: modifiers and a key joined by `+`, as
/// `Primary+Shift+H`. The modifiers are Primary (Command on macOS,
/// Control elsewhere), Ctrl, Shift, Alt and Meta; the key is a
/// letter, a digit, F1 to F12, or a name such as Tab, Enter, Space or
/// Comma. `None` for anything else.
pub fn parse_chord(keys: &str) -> Option<Chord> {
    let mut parts =
        keys.split('+').map(str::trim).collect::<Vec<_>>();
    let key = parse_key(parts.pop()?)?;
    let mut mods = Mods::NONE;
    for part in parts {
        let more = match part.to_ascii_lowercase().as_str() {
            "primary" => Mods::PRIMARY,
            "ctrl" => CTRL,
            "shift" => Mods::SHIFT,
            "alt" => Mods::ALT,
            "meta" => Mods::META,
            _ => return None,
        };
        mods = mods.with(more);
    }
    Some(Chord { key, mods })
}

/// The shell's methods on [`Chord`].
pub trait ChordExt {
    /// The chord as [`parse_chord`] reads it, the same on every
    /// platform. `None` for a key it has no name for.
    fn keys(&self) -> Option<String>;
}

impl ChordExt for Chord {
    fn keys(&self) -> Option<String> {
        let debug = format!("{:?}", self.key);
        let key = ["Key", "Digit", "Arrow"]
            .iter()
            .find_map(|prefix| debug.strip_prefix(prefix))
            .unwrap_or(&debug);
        if parse_key(key) != Some(self.key) {
            return None;
        }
        let mut keys = String::new();
        for (bit, name) in [
            (Mods::PRIMARY, "Primary"),
            (Mods::CTRL, "Ctrl"),
            (Mods::META, "Meta"),
            (Mods::ALT, "Alt"),
            (Mods::SHIFT, "Shift"),
        ] {
            if self.mods.has(bit) {
                keys.push_str(name);
                keys.push('+');
            }
        }
        keys.push_str(key);
        Some(keys)
    }
}

fn parse_key(key: &str) -> Option<KeyCode> {
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
    const FUNCTIONS: [KeyCode; 12] = [
        KeyCode::F1,
        KeyCode::F2,
        KeyCode::F3,
        KeyCode::F4,
        KeyCode::F5,
        KeyCode::F6,
        KeyCode::F7,
        KeyCode::F8,
        KeyCode::F9,
        KeyCode::F10,
        KeyCode::F11,
        KeyCode::F12,
    ];
    let lower = key.to_ascii_lowercase();
    let mut chars = lower.chars();
    if let (Some(one), None) = (chars.next(), chars.next()) {
        return match one {
            'a'..='z' => Some(LETTERS[(one as u8 - b'a') as usize]),
            '0'..='9' => Some(DIGITS[(one as u8 - b'0') as usize]),
            _ => None,
        };
    }
    if let Some(n) = lower
        .strip_prefix('f')
        .and_then(|n| n.parse::<usize>().ok())
    {
        return FUNCTIONS.get(n.checked_sub(1)?).copied();
    }
    Some(match lower.as_str() {
        "tab" => KeyCode::Tab,
        "enter" => KeyCode::Enter,
        "escape" => KeyCode::Escape,
        "space" => KeyCode::Space,
        "backspace" => KeyCode::Backspace,
        "delete" => KeyCode::Delete,
        "up" => KeyCode::ArrowUp,
        "down" => KeyCode::ArrowDown,
        "left" => KeyCode::ArrowLeft,
        "right" => KeyCode::ArrowRight,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        "comma" => KeyCode::Comma,
        "period" => KeyCode::Period,
        "slash" => KeyCode::Slash,
        "backslash" => KeyCode::Backslash,
        "semicolon" => KeyCode::Semicolon,
        "quote" => KeyCode::Quote,
        "backquote" => KeyCode::Backquote,
        "minus" => KeyCode::Minus,
        "equal" => KeyCode::Equal,
        "bracketleft" => KeyCode::BracketLeft,
        "bracketright" => KeyCode::BracketRight,
        _ => return None,
    })
}

/// The engine's own command `name`, which the engine names well.
pub fn engine_id(name: &str) -> CommandId {
    CommandId::new(ENGINE, name).expect("a valid id")
}

/// The shell's methods on [`Commands`].
pub trait ShellCommandsExt {
    /// Spawns the command `id`, with its entry in `menu` and its
    /// keys, which go with it. Its owner observes [`RunCommand`] on
    /// what this returns.
    fn spawn_command(
        &mut self,
        id: CommandId,
        label: &str,
        menu: Option<&str>,
        keys: &[Chord],
    ) -> EntityCommands<'_>;
}

impl ShellCommandsExt for Commands<'_, '_> {
    fn spawn_command(
        &mut self,
        id: CommandId,
        label: &str,
        menu: Option<&str>,
        keys: &[Chord],
    ) -> EntityCommands<'_> {
        let mut command = self.spawn(Command {
            id: id.clone(),
            label: label.into(),
        });
        command.with_children(|kids| {
            if let Some(menu) = menu {
                kids.spawn(MenuEntry {
                    menu: menu.into(),
                    command: id.clone(),
                });
            }
            for &chord in keys {
                kids.spawn(Shortcut {
                    chord,
                    command: id.clone(),
                });
            }
        });
        command
    }
}

/// Runs the command a key press is bound to. A bare key in a text
/// field is the field's to type, and a key being bound to a command
/// runs nothing.
fn press(
    event: On<FocusedInput<KeyboardInput>>,
    windows: Query<(), With<Window>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    focus: Res<InputFocus>,
    typing: Query<(), With<EditableText>>,
    bindings: Res<Bindings>,
    capturing: Res<Capturing>,
    commands: Query<(Entity, &Command)>,
    mut out: Commands,
) {
    let key = &event.input;
    // Seen once, where it starts: at the focus, or at the window
    // when nothing holds the focus.
    let first = match focus.get() {
        Some(held) => event.event_target() == held,
        None => windows.contains(event.event_target()),
    };
    if !first
        || key.state != ButtonState::Pressed
        || key.repeat
        || capturing.active()
    {
        return;
    }
    let mods = keys.as_deref().map_or(Mods::NONE, Mods::held);
    let plain = !(mods.primary || mods.ctrl || mods.alt || mods.meta);
    if plain && focus.get().is_some_and(|held| typing.contains(held))
    {
        return;
    }
    let chord = Chord {
        key: key.key_code,
        mods,
    };
    let bound = bindings.holders(chord).next().and_then(|id| {
        commands.iter().find(|(_, command)| command.id == *id)
    });
    if let Some((entity, _)) = bound {
        out.trigger(RunCommand { entity });
    }
}
