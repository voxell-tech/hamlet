//! The keys, presses, drags and wheel of a terminal's screen: a
//! press gives it the keys, which it keeps from the window's
//! shortcuts; a drag chooses cells to copy; the wheel scrolls back.

use bevy::app::App;
use bevy::clipboard::Clipboard;
use bevy::ecs::entity::Entity;
use bevy::ecs::event::EntityEvent;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::system::{Commands, Local, Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::input::ButtonInput;
use bevy::input::keyboard::{Key, KeyCode, KeyboardInput};
use bevy::input::mouse::MouseScrollUnit;
use bevy::input_focus::{FocusCause, FocusedInput, InputFocus};
use bevy::math::Vec2;
use bevy::picking::events::{
    PointerDrag, PointerPress, PointerScroll,
};
use bevy::picking::pointer::PointerButton;
use bevy::ui::{ComputedNode, UiGlobalTransform};
use hamlet_client::terminals::{
    self, Terminal, TerminalKind, TerminalScreen,
};

use super::Active;
use super::keys::{self, Modes, Mods};
use super::screen::{Canvas, Chosen, Drawn, Span};

/// The lines a notch of the wheel scrolls.
const NOTCH: f32 = 3.0;

pub(super) fn plugin(app: &mut App) {
    app.add_observer(press)
        .add_observer(drag)
        .add_observer(wheel)
        .add_observer(typed);
}

/// The cell under `at`, in the window's logical pixels.
fn cell_at(
    canvas: Entity,
    at: Vec2,
    nodes: &Query<
        (&ComputedNode, &UiGlobalTransform, &Drawn),
        With<Canvas>,
    >,
    screen: &vt100::Screen,
) -> Option<(u16, u16)> {
    let (node, place, drawn) = nodes.get(canvas).ok()?;
    let scale = node.inverse_scale_factor();
    let (_, _, centre) = place.to_scale_angle_translation();
    let corner = (centre - node.size() / 2.0) * scale;
    let inside = at - corner - Vec2::splat(drawn.margin);
    let (rows, cols) = screen.size();
    let col = (inside.x / drawn.cell.x)
        .floor()
        .clamp(0.0, f32::from(cols.get() - 1));
    let row = (inside.y / drawn.cell.y)
        .floor()
        .clamp(0.0, f32::from(rows.get() - 1));
    Some((row as u16, col as u16))
}

/// Gives the screen the keys, and starts choosing cells.
fn press(
    press: On<PointerPress>,
    nodes: Query<
        (&ComputedNode, &UiGlobalTransform, &Drawn),
        With<Canvas>,
    >,
    active: Res<Active>,
    screens: Query<&TerminalScreen>,
    mut focus: ResMut<InputFocus>,
    mut chosen: ResMut<Chosen>,
) {
    if press.entity != press.original_event_target()
        || !nodes.contains(press.entity)
    {
        return;
    }
    if focus.get() != Some(press.entity) {
        focus.set(press.entity, FocusCause::Pressed);
    }
    if press.button != PointerButton::Primary {
        return;
    }
    let spot = active
        .0
        .and_then(|terminal| screens.get(terminal).ok())
        .and_then(|screen| {
            cell_at(
                press.entity,
                press.pointer.position,
                &nodes,
                screen.screen(),
            )
        });
    // A press alone chooses nothing; a drag from it does.
    chosen.0 = None;
    chosen.1 = spot;
}

fn drag(
    drag: On<PointerDrag>,
    nodes: Query<
        (&ComputedNode, &UiGlobalTransform, &Drawn),
        With<Canvas>,
    >,
    active: Res<Active>,
    screens: Query<&TerminalScreen>,
    mut chosen: ResMut<Chosen>,
) {
    if drag.entity != drag.original_event_target()
        || drag.button != PointerButton::Primary
        || !nodes.contains(drag.entity)
    {
        return;
    }
    let (Some(from), Some(screen)) = (
        chosen.1,
        active.0.and_then(|terminal| screens.get(terminal).ok()),
    ) else {
        return;
    };
    let Some(to) = cell_at(
        drag.entity,
        drag.pointer.position,
        &nodes,
        screen.screen(),
    ) else {
        return;
    };
    let span = Some(Span { from, to });
    if chosen.0 != span {
        chosen.0 = span;
    }
}

/// Scrolls back through what the terminal printed. A program that
/// draws the whole screen is sent the arrows instead.
fn wheel(
    scroll: On<PointerScroll>,
    nodes: Query<&Drawn, With<Canvas>>,
    active: Res<Active>,
    mut screens: Query<(&mut TerminalScreen, &Terminal)>,
    mut chosen: ResMut<Chosen>,
    mut left: Local<f32>,
    mut commands: Commands,
) {
    let Ok(drawn) = nodes.get(scroll.entity) else {
        return;
    };
    let Some(terminal) = active.0 else {
        return;
    };
    let Ok((mut screen, view)) = screens.get_mut(terminal) else {
        return;
    };
    let lines = match scroll.unit {
        MouseScrollUnit::Line => scroll.y * NOTCH,
        MouseScrollUnit::Pixel => scroll.y / drawn.cell.y,
    } + *left;
    let whole = lines.trunc();
    *left = lines - whole;
    if whole == 0.0 {
        return;
    }
    let shown = screen.screen();
    if shown.alternate_screen() {
        if view.0.kind == TerminalKind::Shell {
            let arrow = if whole > 0.0 {
                Key::ArrowUp
            } else {
                Key::ArrowDown
            };
            let modes = Modes {
                application_cursor: shown.application_cursor(),
                alt_is_meta: false,
            };
            if let Some(bytes) =
                keys::encode(&arrow, None, Mods::default(), modes)
            {
                let data = bytes.repeat(whole.abs() as usize);
                commands.queue(move |world: &mut World| {
                    terminals::type_in(world, terminal, &data);
                });
            }
        }
        return;
    }
    let back = shown.scrollback() as f32 + whole;
    screen.screen_mut().set_scrollback(back.max(0.0) as usize);
    chosen.0 = None;
}

/// Whether `key` with `mods` is the copy or the paste chord: Command
/// and the letter on macOS, Ctrl, Shift and the letter elsewhere.
fn chord(key: &KeyboardInput, mods: Mods, letter: KeyCode) -> bool {
    let held = if cfg!(target_os = "macos") {
        mods.command && !mods.ctrl && !mods.alt
    } else {
        mods.ctrl && mods.shift && !mods.alt
    };
    held && key.key_code == letter
}

/// The bytes a chord the terminal answers in place of the window
/// types: the macOS editing chords, and Command+K, which clears the
/// screen as Ctrl+L does.
fn edited(key: &KeyboardInput, mods: Mods) -> Option<&'static str> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let clear = mods.command
        && !(mods.ctrl || mods.alt || mods.shift)
        && key.key_code == KeyCode::KeyK;
    if clear {
        return Some("\x0c");
    }
    keys::editing(&key.logical_key, mods)
}

/// Sends the keys typed while the screen has them, and keeps them
/// from the window. Command chords other than copy, paste and the
/// editing ones still reach the window, as does Ctrl+Tab, which goes
/// round the desks.
fn typed(
    mut key: On<FocusedInput<KeyboardInput>>,
    canvases: Query<(), With<Canvas>>,
    held: Option<Res<ButtonInput<KeyCode>>>,
    active: Res<Active>,
    mut chosen: ResMut<Chosen>,
    mut commands: Commands,
) {
    let canvas = key.event_target();
    if canvas != key.original_event_target()
        || !canvases.contains(canvas)
    {
        return;
    }
    let mods = held.as_deref().map_or_else(Mods::default, Mods::held);
    let edit = edited(&key.input, mods);
    let window = mods.command
        && edit.is_none()
        && !chord(&key.input, mods, KeyCode::KeyC)
        && !chord(&key.input, mods, KeyCode::KeyV);
    if window || (mods.ctrl && key.input.key_code == KeyCode::Tab) {
        return;
    }
    key.propagate(false);
    let input = key.input.clone();
    let Some(terminal) = active.0 else {
        return;
    };
    if !input.state.is_pressed() {
        return;
    }
    if let Some(bytes) = edit {
        if chosen.0.is_some() {
            chosen.0 = None;
        }
        commands.queue(move |world: &mut World| {
            terminals::type_in(world, terminal, bytes);
        });
        return;
    }
    if chord(&input, mods, KeyCode::KeyC) {
        let Some(span) = chosen.0 else {
            return;
        };
        commands.queue(move |world: &mut World| {
            copy(world, terminal, span);
        });
        return;
    }
    if chord(&input, mods, KeyCode::KeyV) {
        commands
            .queue(move |world: &mut World| paste(world, terminal));
        return;
    }
    if chosen.0.is_some() {
        chosen.0 = None;
    }
    commands.queue(move |world: &mut World| {
        let Some(mut screen) =
            world.get_mut::<TerminalScreen>(terminal)
        else {
            return;
        };
        let modes = Modes {
            application_cursor: screen.screen().application_cursor(),
            alt_is_meta: !cfg!(target_os = "macos"),
        };
        let text = input.text.as_deref();
        let Some(bytes) =
            keys::encode(&input.logical_key, text, mods, modes)
        else {
            return;
        };
        if screen.screen().scrollback() > 0 {
            screen.screen_mut().set_scrollback(0);
        }
        terminals::type_in(world, terminal, &bytes);
    });
}

fn copy(world: &mut World, terminal: Entity, span: Span) {
    let Some(text) = world
        .get::<TerminalScreen>(terminal)
        .map(|screen| span.text(screen.screen()))
    else {
        return;
    };
    if let Some(mut clipboard) = world.get_resource_mut::<Clipboard>()
        && let Err(e) = clipboard.set_text(text)
    {
        bevy::log::warn!("could not copy from the terminal: {e:?}");
    }
}

fn paste(world: &mut World, terminal: Entity) {
    let Some(mut clipboard) = world.get_resource_mut::<Clipboard>()
    else {
        return;
    };
    let Some(Ok(text)) = clipboard.fetch_text().poll_result() else {
        return;
    };
    let bracketed = world
        .get::<TerminalScreen>(terminal)
        .is_some_and(|screen| screen.screen().bracketed_paste());
    terminals::type_in(world, terminal, &pasted(&text, bracketed));
}

/// `text` as a paste is typed: its line ends as Enter, and marked as
/// a paste for a program that asked.
fn pasted(text: &str, bracketed: bool) -> String {
    let text = text.replace("\r\n", "\r").replace('\n', "\r");
    if bracketed {
        // A paste that holds the end mark could end the paste early.
        let text = text.replace("\x1b[201~", "");
        format!("\x1b[200~{text}\x1b[201~")
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_paste_ends_lines_with_enter_and_is_marked_when_asked() {
        assert_eq!(pasted("a\r\nb\nc", false), "a\rb\rc");
        assert_eq!(
            pasted("x\x1b[201~y", true),
            "\x1b[200~xy\x1b[201~"
        );
    }
}
