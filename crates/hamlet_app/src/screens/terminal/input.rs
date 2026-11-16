//! The keys, presses, drags and wheel of a terminal's screen: a
//! press gives it the keys, which it keeps from the window's
//! shortcuts; a drag chooses cells to copy; the wheel scrolls back.
//! While the program in a shell reads the mouse, presses, drags,
//! moves and the wheel go to it instead, unless Shift is held, which
//! chooses cells as before.

use bevy::app::{App, Update};
use bevy::clipboard::Clipboard;
use bevy::ecs::entity::Entity;
use bevy::ecs::event::EntityEvent;
use bevy::ecs::message::MessageReader;
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
    PointerDrag, PointerDragEnd, PointerMove, PointerPress,
    PointerRelease, PointerScroll,
};
use bevy::picking::pointer::PointerButton;
use bevy::ui::{ComputedNode, UiGlobalTransform};
use bevy::window::WindowFocused;
use hamlet_client::terminals::{
    self, Terminal, TerminalKind, TerminalScreen,
};

use super::Panels;
use super::keys::{self, Modes, Mods};
use super::mouse::{self, Button, Report};
use super::screen::{Canvas, Chosen, Drawn, Reporting, Span};

/// The lines a notch of the wheel scrolls.
const NOTCH: f32 = 3.0;

pub(super) fn plugin(app: &mut App) {
    app.add_observer(press)
        .add_observer(drag)
        .add_observer(release)
        .add_observer(drag_end)
        .add_observer(hover)
        .add_observer(wheel)
        .add_observer(typed)
        .add_systems(Update, let_go_on_blur);
}

/// The canvas nodes a pointer's cell is read from.
type Nodes<'w, 's> = Query<
    'w,
    's,
    (
        &'static ComputedNode,
        &'static UiGlobalTransform,
        &'static Drawn,
    ),
    With<Canvas>,
>;

/// The cell under `at`, in the window's logical pixels.
fn cell_at(
    canvas: Entity,
    at: Vec2,
    nodes: &Nodes,
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

fn button_of(button: PointerButton) -> Button {
    match button {
        PointerButton::Primary => Button::Left,
        PointerButton::Secondary => Button::Right,
        PointerButton::Middle => Button::Middle,
    }
}

fn mods_of(held: Option<&ButtonInput<KeyCode>>) -> Mods {
    held.map_or_else(Mods::default, Mods::held)
}

/// What a canvas's pointer reaches: its terminal, and its program's
/// mouse modes while the shell's program reads the mouse and Shift is
/// not held.
struct Target {
    terminal: Entity,
    mode: vt100::MouseProtocolMode,
    encoding: vt100::MouseProtocolEncoding,
}

fn target(
    panels: &Panels,
    canvas: &Canvas,
    screens: &Query<(&TerminalScreen, &Terminal)>,
    mods: Mods,
) -> Option<Target> {
    let terminal = panels.active(&canvas.0)?;
    let (screen, view) = screens.get(terminal).ok()?;
    let screen = screen.screen();
    let mode = screen.mouse_protocol_mode();
    let read = view.0.kind == TerminalKind::Shell
        && mode != vt100::MouseProtocolMode::None
        && !mods.shift
        && screen.scrollback() == 0;
    read.then_some(Target {
        terminal,
        mode,
        encoding: screen.mouse_protocol_encoding(),
    })
}

/// Tells `target`'s program of `report` at `cell`, if it asks.
fn report(
    commands: &mut Commands,
    target: &Target,
    report: Report,
    cell: (u16, u16),
    mods: Mods,
) {
    if !mouse::wanted(target.mode, report) {
        return;
    }
    if let Some(text) =
        mouse::encode(report, cell, mods, target.encoding)
    {
        let terminal = target.terminal;
        commands.queue(move |world: &mut World| {
            terminals::type_in(world, terminal, &text);
        });
    }
}

/// Gives the screen the keys, and starts choosing cells, or tells the
/// program the button went down.
#[expect(
    clippy::too_many_arguments,
    reason = "a press reads the canvas, its terminal and the keys \
              held"
)]
fn press(
    press: On<PointerPress>,
    nodes: Nodes,
    mut canvases: Query<(&Canvas, &mut Chosen, &mut Reporting)>,
    panels: Res<Panels>,
    screens: Query<(&TerminalScreen, &Terminal)>,
    held: Option<Res<ButtonInput<KeyCode>>>,
    mut focus: ResMut<InputFocus>,
    mut commands: Commands,
) {
    // What was hit may be inside the canvas; the press reaches it
    // as it goes up.
    if !nodes.contains(press.entity) {
        return;
    }
    let Ok((canvas, mut chosen, mut reporting)) =
        canvases.get_mut(press.entity)
    else {
        return;
    };
    if focus.get() != Some(press.entity) {
        focus.set(press.entity, FocusCause::Pressed);
    }
    let mods = mods_of(held.as_deref());
    if let Some(target) = target(&panels, canvas, &screens, mods) {
        let Ok((screen, _)) = screens.get(target.terminal) else {
            return;
        };
        let Some(cell) = cell_at(
            press.entity,
            press.pointer.position,
            &nodes,
            screen.screen(),
        ) else {
            return;
        };
        let button = button_of(press.button);
        report(
            &mut commands,
            &target,
            Report::Press(button),
            cell,
            mods,
        );
        *reporting = Reporting {
            held: Some(button),
            cell: Some(cell),
        };
        *chosen = Chosen::default();
        return;
    }
    if press.button != PointerButton::Primary {
        return;
    }
    let spot = panels
        .active(&canvas.0)
        .and_then(|terminal| screens.get(terminal).ok())
        .and_then(|(screen, _)| {
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

/// Chooses cells as the pointer is dragged, or tells the program the
/// pointer moved with its button held.
fn drag(
    drag: On<PointerDrag>,
    nodes: Nodes,
    mut canvases: Query<(&Canvas, &mut Chosen, &mut Reporting)>,
    panels: Res<Panels>,
    screens: Query<(&TerminalScreen, &Terminal)>,
    held: Option<Res<ButtonInput<KeyCode>>>,
    mut commands: Commands,
) {
    if !nodes.contains(drag.entity) {
        return;
    }
    let Ok((canvas, mut chosen, mut reporting)) =
        canvases.get_mut(drag.entity)
    else {
        return;
    };
    let Some((screen, _)) = panels
        .active(&canvas.0)
        .and_then(|terminal| screens.get(terminal).ok())
    else {
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
    let mods = mods_of(held.as_deref());
    if let Some(button) = reporting.held {
        if reporting.cell != Some(to)
            && let Some(target) =
                target(&panels, canvas, &screens, mods)
        {
            report(
                &mut commands,
                &target,
                Report::Moved(Some(button)),
                to,
                mods,
            );
            reporting.cell = Some(to);
        }
        return;
    }
    if drag.button != PointerButton::Primary {
        return;
    }
    let Some(from) = chosen.1 else {
        return;
    };
    let span = Some(Span { from, to });
    if chosen.0 != span {
        chosen.0 = span;
    }
}

/// Tells the program the button it was told of went up, at `cell`.
fn let_go(
    commands: &mut Commands,
    reporting: &mut Reporting,
    target: Option<Target>,
    cell: Option<(u16, u16)>,
    mods: Mods,
) {
    let (Some(button), Some(target), Some(cell)) =
        (reporting.held, target, cell.or(reporting.cell))
    else {
        *reporting = Reporting::default();
        return;
    };
    report(commands, &target, Report::Release(button), cell, mods);
    *reporting = Reporting::default();
}

/// Tells the program a button went up over the screen.
fn release(
    release: On<PointerRelease>,
    nodes: Nodes,
    mut canvases: Query<(&Canvas, &mut Reporting)>,
    panels: Res<Panels>,
    screens: Query<(&TerminalScreen, &Terminal)>,
    held: Option<Res<ButtonInput<KeyCode>>>,
    mut commands: Commands,
) {
    let Ok((canvas, mut reporting)) =
        canvases.get_mut(release.entity)
    else {
        return;
    };
    if reporting.held != Some(button_of(release.button)) {
        return;
    }
    let mods = mods_of(held.as_deref());
    let target = target(&panels, canvas, &screens, Mods::default());
    let cell = target.as_ref().and_then(|target| {
        let (screen, _) = screens.get(target.terminal).ok()?;
        cell_at(
            release.entity,
            release.pointer.position,
            &nodes,
            screen.screen(),
        )
    });
    let_go(&mut commands, &mut reporting, target, cell, mods);
}

/// Tells the program a button went up once a drag that left the
/// screen ends, at the last cell it was told of.
fn drag_end(
    end: On<PointerDragEnd>,
    mut canvases: Query<(&Canvas, &mut Reporting)>,
    panels: Res<Panels>,
    screens: Query<(&TerminalScreen, &Terminal)>,
    held: Option<Res<ButtonInput<KeyCode>>>,
    mut commands: Commands,
) {
    let Ok((canvas, mut reporting)) = canvases.get_mut(end.entity)
    else {
        return;
    };
    if reporting.held != Some(button_of(end.button)) {
        return;
    }
    let target = target(&panels, canvas, &screens, Mods::default());
    let mods = mods_of(held.as_deref());
    let_go(&mut commands, &mut reporting, target, None, mods);
}

/// Tells a program that reads every move where the pointer is.
fn hover(
    moved: On<PointerMove>,
    nodes: Nodes,
    mut canvases: Query<(&Canvas, &mut Reporting)>,
    panels: Res<Panels>,
    screens: Query<(&TerminalScreen, &Terminal)>,
    held: Option<Res<ButtonInput<KeyCode>>>,
    mut commands: Commands,
) {
    let Ok((canvas, mut reporting)) = canvases.get_mut(moved.entity)
    else {
        return;
    };
    if reporting.held.is_some() {
        return;
    }
    let mods = mods_of(held.as_deref());
    let Some(target) = target(&panels, canvas, &screens, mods) else {
        return;
    };
    let Ok((screen, _)) = screens.get(target.terminal) else {
        return;
    };
    let Some(cell) = cell_at(
        moved.entity,
        moved.pointer.position,
        &nodes,
        screen.screen(),
    ) else {
        return;
    };
    if reporting.cell != Some(cell) {
        report(
            &mut commands,
            &target,
            Report::Moved(None),
            cell,
            mods,
        );
        reporting.cell = Some(cell);
    }
}

/// Lets go of what a press held once the window loses the keys: the
/// release goes to another window then, so the program is told now,
/// and a drag choosing cells ends where it was.
fn let_go_on_blur(
    mut focused: MessageReader<WindowFocused>,
    mut canvases: Query<(&Canvas, &mut Chosen, &mut Reporting)>,
    panels: Res<Panels>,
    screens: Query<(&TerminalScreen, &Terminal)>,
    mut commands: Commands,
) {
    if !focused.read().any(|event| !event.focused) {
        return;
    }
    for (canvas, mut chosen, mut reporting) in &mut canvases {
        if chosen.1.is_some() {
            chosen.1 = None;
        }
        if reporting.held.is_some() {
            let target =
                target(&panels, canvas, &screens, Mods::default());
            let_go(
                &mut commands,
                &mut reporting,
                target,
                None,
                Mods::default(),
            );
        }
    }
}

/// Scrolls back through what the terminal printed. A program that
/// reads the mouse is sent the wheel, and one that draws the whole
/// screen otherwise the arrows.
#[expect(
    clippy::too_many_arguments,
    reason = "the wheel reads the canvas, its terminal and the keys \
              held"
)]
fn wheel(
    scroll: On<PointerScroll>,
    nodes: Nodes,
    mut canvases: Query<(&Canvas, &mut Chosen)>,
    panels: Res<Panels>,
    mut screens: Query<(&mut TerminalScreen, &Terminal)>,
    held: Option<Res<ButtonInput<KeyCode>>>,
    mut left: Local<f32>,
    mut commands: Commands,
) {
    let Ok((_, _, drawn)) = nodes.get(scroll.entity) else {
        return;
    };
    let Ok((canvas, mut chosen)) = canvases.get_mut(scroll.entity)
    else {
        return;
    };
    let Some(terminal) = panels.active(&canvas.0) else {
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
    let mods = mods_of(held.as_deref());
    let shell = view.0.kind == TerminalKind::Shell;
    let mode = shown.mouse_protocol_mode();
    if shell
        && mode != vt100::MouseProtocolMode::None
        && !mods.shift
        && shown.scrollback() == 0
    {
        let Some(cell) = cell_at(
            scroll.entity,
            scroll.pointer.position,
            &nodes,
            shown,
        ) else {
            return;
        };
        let up = whole > 0.0;
        let Some(notch) = mouse::encode(
            Report::Wheel { up },
            cell,
            mods,
            shown.mouse_protocol_encoding(),
        ) else {
            return;
        };
        let data =
            notch.repeat((whole.abs() / NOTCH).ceil() as usize);
        commands.queue(move |world: &mut World| {
            terminals::type_in(world, terminal, &data);
        });
        return;
    }
    if shown.alternate_screen() {
        if shell {
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
    mut canvases: Query<(&Canvas, &mut Chosen)>,
    held: Option<Res<ButtonInput<KeyCode>>>,
    panels: Res<Panels>,
    mut commands: Commands,
) {
    let target = key.event_target();
    if target != key.original_event_target() {
        return;
    }
    let Ok((canvas, mut chosen)) = canvases.get_mut(target) else {
        return;
    };
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
    let Some(terminal) = panels.active(&canvas.0) else {
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
        // Without the escapes that begin a sequence, a paste cannot
        // end itself early or type a sequence in: ESC, and the C1
        // CSI. After ratty's `encode_paste` (github.com/orhun/ratty).
        let text = text
            .chars()
            .filter(|c| !matches!(c, '\x1b' | '\u{9b}'))
            .collect::<String>();
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
            pasted("x\x1b[201~y\u{9b}31mz", true),
            "\x1b[200~x[201~y31mz\x1b[201~"
        );
    }
}

#[cfg(test)]
mod reporting {
    use bevy::ecs::system::SystemState;
    use hamlet_client::outbox::Outbox;
    use hamlet_protocol::client as protocol;
    use serde_json::json;

    use super::*;
    use crate::screens::terminal::PanelId;

    fn view(id: &str, kind: &str) -> terminals::TerminalView {
        serde_json::from_value(json!({
            "agent": null, "cols": 80, "rows": 24, "cwd": "/",
            "exit": null, "hamlet": null, "id": id, "kind": kind,
            "name": id, "title": ""
        }))
        .unwrap()
    }

    /// Presses at row 1, column 2 of what panel `panel` shows, with
    /// `mods` held, and gives what the shell was typed.
    fn press_at(
        world: &mut World,
        panel: &PanelId,
        mods: Mods,
    ) -> Vec<String> {
        let mut state = SystemState::<(
            Res<Panels>,
            Query<(&TerminalScreen, &Terminal)>,
            Commands,
        )>::new(world);
        let (panels, screens, mut commands) =
            state.get_mut(world).unwrap();
        if let Some(target) =
            target(&panels, &Canvas(panel.clone()), &screens, mods)
        {
            let press = Report::Press(Button::Left);
            report(&mut commands, &target, press, (1, 2), mods);
        }
        state.apply(world);
        world
            .resource_mut::<Outbox>()
            .drain()
            .filter_map(|message| match message {
                protocol::ClientMessage::Terminal(
                    protocol::Terminal::Input { data, .. },
                ) => Some(data),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_press_reaches_a_shells_program_that_reads_the_mouse_unless_shift_is_held()
     {
        let mut app = crate::testing::panel_app();
        let mut state = hamlet_testing::realm::state();
        state.revision = 1;
        state.terminals =
            vec![view("t1", "shell"), view("t2", "commands")];
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        app.init_resource::<Panels>();
        let world = app.world_mut();
        let (shell, commands) = (
            terminals::find(world, "t1").unwrap(),
            terminals::find(world, "t2").unwrap(),
        );
        let (first, second) =
            (PanelId::first(), PanelId("terminal-2".into()));
        for (panel, terminal) in
            [(&first, shell), (&second, commands)]
        {
            terminals::watch(world, terminal);
            world.resource_mut::<Panels>().edit(panel).active =
                Some(terminal);
        }
        world.resource_mut::<Outbox>().drain().for_each(drop);
        let none = Mods::default();
        assert!(press_at(world, &first, none).is_empty());
        for terminal in [shell, commands] {
            world
                .get_mut::<TerminalScreen>(terminal)
                .unwrap()
                .process(b"\x1b[?1000h\x1b[?1006h");
        }
        assert_eq!(press_at(world, &first, none), ["\x1b[<0;3;2M"]);
        let shift = Mods {
            shift: true,
            ..none
        };
        assert!(press_at(world, &first, shift).is_empty());
        // An agent's commands take no input.
        assert!(press_at(world, &second, none).is_empty());
    }
}
