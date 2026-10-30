//! The terminals the server runs, and the screens of those the window
//! watches.

use std::collections::HashMap;
use std::num::NonZeroU16;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use bevy_app::App;
use bevy_ecs::prelude::*;
use bevy_log::warn;
use hamlet_protocol::{
    client as protocol, screen as protocol_screen,
};
pub use protocol::{TERMINAL_SIZE, TerminalKind, TerminalView};

use crate::index::{self, Index};
use crate::messages::{Reconnected, Response};
use crate::outbox::{Outbox, RequestId};

/// The lines a watched terminal keeps above its screen.
const SCROLLBACK: usize = 5000;

/// The most bytes one input message carries.
const INPUT_MOST: usize = 65_536;

pub(crate) fn plugin(app: &mut App) {
    let world = app.world_mut();
    world.register_component::<TerminalId>();
    world.register_component::<Terminal>();
    world.register_component::<Watched>();
    world.register_component::<TerminalScreen>();
    app.init_resource::<Opening>()
        .add_observer(rewatch)
        .add_observer(refused);
}

/// A terminal's server id.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
#[component(
    immutable,
    on_insert = index::file_terminal,
    on_discard = index::unfile_terminal
)]
pub struct TerminalId(pub String);

/// A terminal as the server last listed it.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct Terminal(pub TerminalView);

/// On a terminal the window watches.
#[derive(Component, Clone, Copy, Debug)]
pub struct Watched;

/// What a watched terminal shows, kept by its emulator.
#[derive(Component)]
pub struct TerminalScreen {
    parser: vt100::Parser,
}

impl TerminalScreen {
    fn new(cols: u16, rows: u16) -> Self {
        Self {
            parser: vt100::Parser::new(
                fit(rows),
                fit(cols),
                SCROLLBACK,
            ),
        }
    }

    pub fn screen(&self) -> &vt100::Screen {
        self.parser.screen()
    }

    pub fn screen_mut(&mut self) -> &mut vt100::Screen {
        self.parser.screen_mut()
    }

    /// Feeds the emulator bytes the terminal printed.
    pub fn process(&mut self, bytes: &[u8]) {
        self.parser.process(bytes);
    }

    /// The emulator, blank at `cols` by `rows`.
    pub fn reset(&mut self, cols: u16, rows: u16) {
        *self = Self::new(cols, rows);
    }

    fn set_size(&mut self, cols: u16, rows: u16) {
        let (now_rows, now_cols) = self.parser.screen().size();
        if (now_cols.get(), now_rows.get()) != (cols, rows) {
            protocol_screen::resize(
                &mut self.parser,
                fit(rows),
                fit(cols),
            );
        }
    }
}

/// A terminal opened by the window, with the open it answers.
#[derive(Event, Clone, Debug)]
pub struct TerminalOpened {
    pub request: RequestId,
    pub terminal: Entity,
}

/// The size of each open in flight, until its first output names its
/// terminal.
#[derive(Resource, Default)]
struct Opening(HashMap<RequestId, (u16, u16)>);

fn fit(size: u16) -> NonZeroU16 {
    NonZeroU16::new(size.clamp(1, TERMINAL_SIZE))
        .unwrap_or(NonZeroU16::MIN)
}

/// A terminal by its server id.
pub fn find(world: &World, id: &str) -> Option<Entity> {
    world.get_resource::<Index>()?.terminal(id)
}

fn id_of(world: &World, terminal: Entity) -> Option<String> {
    world.get::<TerminalId>(terminal).map(|id| id.0.clone())
}

/// Opens a shell in `hamlet`, or an unbounded one without. Its first
/// output triggers [`TerminalOpened`] with the returned id.
pub fn open(
    world: &mut World,
    hamlet: Option<String>,
    cols: u16,
    rows: u16,
) -> RequestId {
    let (cols, rows) = (fit(cols).get(), fit(rows).get());
    let request = world.resource_mut::<Outbox>().send(|id| {
        protocol::ClientMessage::Terminal(protocol::Terminal::Open {
            cols,
            hamlet,
            id,
            rows,
        })
    });
    world
        .resource_mut::<Opening>()
        .0
        .insert(request.clone(), (cols, rows));
    request
}

/// Starts watching `terminal`, from a blank screen until the server
/// sends what it shows.
pub fn watch(world: &mut World, terminal: Entity) {
    if world.get::<Watched>(terminal).is_some() {
        return;
    }
    let Some(id) = id_of(world, terminal) else {
        return;
    };
    let (cols, rows) = world
        .get::<Terminal>(terminal)
        .map_or((80, 24), |view| (view.0.cols, view.0.rows));
    world
        .entity_mut(terminal)
        .insert((Watched, TerminalScreen::new(cols, rows)));
    send_watch(&mut world.resource_mut::<Outbox>(), id);
}

fn send_watch(outbox: &mut Outbox, terminal: String) {
    outbox.send(|id| {
        protocol::ClientMessage::Terminal(protocol::Terminal::Watch {
            id,
            terminal,
        })
    });
}

/// Stops watching `terminal` and lets go of its screen.
pub fn unwatch(world: &mut World, terminal: Entity) {
    if world.get::<Watched>(terminal).is_none() {
        return;
    }
    world
        .entity_mut(terminal)
        .remove::<(Watched, TerminalScreen)>();
    let Some(terminal) = id_of(world, terminal) else {
        return;
    };
    world.resource_mut::<Outbox>().send(|_| {
        protocol::ClientMessage::Terminal(
            protocol::Terminal::Unwatch { terminal },
        )
    });
}

/// Types `data` into a shell. A terminal of runs takes nothing.
pub fn type_in(world: &mut World, terminal: Entity, data: &str) {
    let shell = world
        .get::<Terminal>(terminal)
        .is_some_and(|view| view.0.kind == TerminalKind::Shell);
    let Some(id) = id_of(world, terminal).filter(|_| shell) else {
        return;
    };
    let mut outbox = world.resource_mut::<Outbox>();
    for data in chunks(data, INPUT_MOST) {
        outbox.send(|_| {
            protocol::ClientMessage::Terminal(
                protocol::Terminal::Input {
                    data: data.to_owned(),
                    terminal: id.clone(),
                },
            )
        });
    }
}

/// `text` in pieces of at most `most` bytes, cut between characters.
fn chunks(text: &str, most: usize) -> impl Iterator<Item = &str> {
    let mut rest = text;
    std::iter::from_fn(move || {
        if rest.is_empty() {
            return None;
        }
        let mut cut = rest.len().min(most);
        while !rest.is_char_boundary(cut) {
            cut -= 1;
        }
        let (piece, after) = rest.split_at(cut);
        rest = after;
        Some(piece)
    })
}

/// Gives `terminal` a new size, here and on the server.
pub fn resize(
    world: &mut World,
    terminal: Entity,
    cols: u16,
    rows: u16,
) {
    let (cols, rows) = (fit(cols).get(), fit(rows).get());
    let Some(id) = id_of(world, terminal) else {
        return;
    };
    if let Some(mut screen) =
        world.get_mut::<TerminalScreen>(terminal)
    {
        screen.set_size(cols, rows);
    }
    world.resource_mut::<Outbox>().send(|_| {
        protocol::ClientMessage::Terminal(
            protocol::Terminal::Resize {
                cols,
                rows,
                terminal: id,
            },
        )
    });
}

/// Ends `terminal`'s processes. The server removes it.
pub fn close(
    world: &mut World,
    terminal: Entity,
) -> Option<RequestId> {
    let terminal = id_of(world, terminal)?;
    Some(world.resource_mut::<Outbox>().send(|id| {
        protocol::ClientMessage::Terminal(protocol::Terminal::Close {
            id,
            terminal,
        })
    }))
}

/// Names `terminal` `name`, shown over the title its programs set.
pub fn rename(
    world: &mut World,
    terminal: Entity,
    name: &str,
) -> Option<RequestId> {
    let terminal = id_of(world, terminal)?;
    let name = name.trim().chars().take(64).collect::<String>();
    if name.is_empty() {
        return None;
    }
    Some(world.resource_mut::<Outbox>().send(|id| {
        protocol::ClientMessage::Terminal(
            protocol::Terminal::Rename { id, name, terminal },
        )
    }))
}

/// Puts every terminal of a whole state in, and takes out those it
/// lacks.
pub(crate) fn state(world: &mut World, terminals: Vec<TerminalView>) {
    let kept = terminals
        .into_iter()
        .map(|view| upsert(world, view))
        .collect::<Vec<_>>();
    let gone = world
        .resource::<Index>()
        .terminals()
        // One the window opened may be named before it is listed.
        .filter(|entity| {
            !kept.contains(entity)
                && world.get::<Terminal>(*entity).is_some()
        })
        .collect::<Vec<_>>();
    for entity in gone {
        world.despawn(entity);
    }
}

pub(crate) fn upsert(
    world: &mut World,
    view: TerminalView,
) -> Entity {
    let entity = find(world, &view.id).unwrap_or_else(|| {
        world.spawn(TerminalId(view.id.clone())).id()
    });
    let mut terminal = world.entity_mut(entity);
    if let Some(mut screen) = terminal.get_mut::<TerminalScreen>() {
        screen.set_size(view.cols, view.rows);
    }
    if terminal.get::<Terminal>().map(|was| &was.0) != Some(&view) {
        terminal.insert(Terminal(view));
    }
    entity
}

pub(crate) fn gone(world: &mut World, id: &str) {
    if let Some(entity) = find(world, id) {
        world.despawn(entity);
    }
}

/// Feeds a watched terminal's screen what it printed.
pub(crate) fn receive(
    world: &mut World,
    output: protocol::TerminalOutput,
) {
    let opened = output.to.as_ref().and_then(|to| {
        let size = world.resource_mut::<Opening>().0.remove(to)?;
        Some((to.clone(), size))
    });
    let entity = match (find(world, &output.terminal), &opened) {
        (Some(entity), _) => entity,
        (None, Some(_)) => {
            world.spawn(TerminalId(output.terminal.clone())).id()
        }
        (None, None) => return,
    };
    if let Some((_, (cols, rows))) = opened
        && world.get::<Watched>(entity).is_none()
    {
        // The server watches what it opens for the window that asked.
        world
            .entity_mut(entity)
            .insert((Watched, TerminalScreen::new(cols, rows)));
    }
    let size = world
        .get::<Terminal>(entity)
        .map(|view| (view.0.cols, view.0.rows));
    match STANDARD.decode(&output.data) {
        Ok(bytes) => {
            if let Some(mut screen) =
                world.get_mut::<TerminalScreen>(entity)
            {
                if output.reset {
                    let (rows, cols) = screen.screen().size();
                    let (cols, rows) =
                        size.unwrap_or((cols.get(), rows.get()));
                    screen.reset(cols, rows);
                    // Each part at the size it was printed at, so the
                    // screen is drawn as it was, not wrapped anew.
                    for part in protocol::replayed(&bytes) {
                        match part {
                            protocol::Replayed::Printed(bytes) => {
                                screen.process(bytes);
                            }
                            protocol::Replayed::Sized {
                                cols,
                                rows,
                            } => {
                                screen.set_size(cols, rows);
                            }
                        }
                    }
                } else {
                    screen.process(&bytes);
                }
            }
        }
        Err(e) => warn!("terminal {}: {e}", output.terminal),
    }
    if let Some((request, _)) = opened {
        world.trigger(TerminalOpened {
            request,
            terminal: entity,
        });
    }
}

/// Watches every watched terminal again on a new connection.
fn rewatch(
    _: On<Reconnected>,
    watched: Query<&TerminalId, With<Watched>>,
    mut outbox: ResMut<Outbox>,
    mut opening: ResMut<Opening>,
) {
    opening.0.clear();
    for id in &watched {
        send_watch(&mut outbox, id.0.clone());
    }
}

/// Forgets an open the server refused.
fn refused(response: On<Response>, mut opening: ResMut<Opening>) {
    if matches!(
        &response.message,
        protocol::ServerMessage::Reply(reply)
            if reply.kind == protocol::ReplyKind::Error
    ) || matches!(
        response.message,
        protocol::ServerMessage::Fault(_)
    ) {
        opening.0.remove(&response.to);
    }
}

#[cfg(test)]
mod tests {
    use bevy_app::App;
    use hamlet_protocol::client as protocol;
    use protocol::TerminalOutput;
    use serde_json::json;

    use super::*;
    use crate::{ClientPlugin, receive};

    fn view(id: &str, kind: &str) -> TerminalView {
        serde_json::from_value(json!({
            "agent": null, "cols": 20, "rows": 4, "cwd": "/",
            "exit": null, "hamlet": "home", "id": id, "kind": kind,
            "name": id, "sandboxed": true, "title": ""
        }))
        .unwrap()
    }

    fn output(
        terminal: &str,
        text: &str,
        reset: bool,
    ) -> TerminalOutput {
        TerminalOutput {
            data: STANDARD.encode(text),
            reset,
            terminal: terminal.into(),
            to: None,
        }
    }

    fn app(terminals: Vec<TerminalView>) -> App {
        let mut app = App::new();
        app.add_plugins(ClientPlugin { server: None });
        let mut state = hamlet_testing::realm::state();
        state.terminals = terminals;
        receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        app
    }

    fn sent(app: &mut App) -> Vec<protocol::ClientMessage> {
        app.world_mut().resource_mut::<Outbox>().drain().collect()
    }

    fn row(app: &App, terminal: Entity, row: u16) -> String {
        let screen =
            app.world().get::<TerminalScreen>(terminal).unwrap();
        screen.screen().rows(0, 80).nth(row.into()).unwrap()
    }

    #[test]
    fn input_is_cut_between_characters_into_pieces_the_server_takes()
    {
        let text = "ab\u{e9}cd";
        let pieces = chunks(text, 3).collect::<Vec<_>>();
        assert_eq!(pieces, ["ab", "\u{e9}c", "d"]);
        assert_eq!(chunks("", 3).count(), 0);
    }

    #[test]
    fn a_reset_redraws_the_screen_from_blank_and_later_output_adds_to_it()
     {
        let mut app = app(vec![view("t1", "shell")]);
        let terminal = find(app.world(), "t1").unwrap();
        watch(app.world_mut(), terminal);
        let world = app.world_mut();
        receive(
            world,
            protocol::ServerMessage::Terminal(output(
                "t1", "old", true,
            )),
        );
        receive(
            world,
            protocol::ServerMessage::Terminal(output(
                "t1", "new", true,
            )),
        );
        receive(
            world,
            protocol::ServerMessage::Terminal(output(
                "t1", "er", false,
            )),
        );
        assert_eq!(row(&app, terminal, 0), "newer");
    }

    #[test]
    fn a_redraw_draws_each_part_at_the_size_it_was_printed_at() {
        let mut app = app(vec![view("t1", "shell")]);
        let terminal = find(app.world(), "t1").unwrap();
        watch(app.world_mut(), terminal);
        let mut bytes = protocol::size_mark(4, 3);
        bytes.extend_from_slice(b"abcdef");
        bytes.extend(protocol::size_mark(8, 3));
        receive(
            app.world_mut(),
            protocol::ServerMessage::Terminal(TerminalOutput {
                data: STANDARD.encode(bytes),
                reset: true,
                terminal: "t1".into(),
                to: None,
            }),
        );
        // Printed at 4 columns, then wrapped again at 8.
        assert_eq!(row(&app, terminal, 0), "abcdef");
        assert_eq!(row(&app, terminal, 1), "");
        let screen =
            app.world().get::<TerminalScreen>(terminal).unwrap();
        assert_eq!(screen.screen().size().1.get(), 8);
    }

    #[test]
    fn the_answer_to_an_open_names_a_watched_terminal_before_it_is_listed()
     {
        let mut app = app(vec![]);
        let request = open(app.world_mut(), None, 30, 5);
        let opened = std::sync::Arc::new(std::sync::Mutex::new(None));
        let seen = opened.clone();
        app.world_mut().add_observer(
            move |at: On<TerminalOpened>| {
                *seen.lock().unwrap() =
                    Some((at.request.clone(), at.terminal));
            },
        );
        let mut first = output("t9", "$ ", true);
        first.to = Some(request.clone());
        receive(
            app.world_mut(),
            protocol::ServerMessage::Terminal(first),
        );
        let terminal = find(app.world(), "t9").unwrap();
        assert_eq!(
            *opened.lock().unwrap(),
            Some((request, terminal))
        );
        assert_eq!(row(&app, terminal, 0), "$ ");
        // A whole state that does not list it yet keeps it.
        let mut state = hamlet_testing::realm::state();
        state.revision = 1;
        receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        assert_eq!(find(app.world(), "t9"), Some(terminal));
    }

    #[test]
    fn watched_terminals_are_watched_again_on_a_new_connection_and_gone_ones_removed()
     {
        let mut app =
            app(vec![view("t1", "shell"), view("t2", "runs")]);
        let t1 = find(app.world(), "t1").unwrap();
        watch(app.world_mut(), t1);
        sent(&mut app);
        app.world_mut().trigger(Reconnected);
        let again = sent(&mut app);
        assert!(matches!(
            again.as_slice(),
            [protocol::ClientMessage::Terminal(
                protocol::Terminal::Watch { terminal, .. }
            )] if terminal == "t1"
        ));
        let mut state = hamlet_testing::realm::state();
        state.revision = 1;
        state.terminals = vec![view("t2", "runs")];
        receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        assert_eq!(find(app.world(), "t1"), None);
        assert!(app.world().get_entity(t1).is_err());
    }

    #[test]
    fn a_terminal_of_runs_takes_no_input() {
        let mut app =
            app(vec![view("t1", "shell"), view("t2", "runs")]);
        let (t1, t2) = (
            find(app.world(), "t1").unwrap(),
            find(app.world(), "t2").unwrap(),
        );
        type_in(app.world_mut(), t2, "ls\r");
        assert!(sent(&mut app).is_empty());
        type_in(app.world_mut(), t1, "ls\r");
        assert_eq!(sent(&mut app).len(), 1);
    }
}
