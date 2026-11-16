//! Terminals: the chief's own shells on pseudo-terminals, and the
//! commands of each agent's shell tool, kept for the windows that
//! watch them and across restarts.
//!
//! A shell is the chief's own, with the server account's reach and
//! no sandbox; agents have none. An agent's commands terminal shows
//! what its shell tool ran, and takes no input.
//!
//! What a terminal prints is queued, and the first print into an
//! empty queue wakes the runner, so one input takes in a burst of
//! output.

pub(crate) mod chief;
pub(crate) mod kept;
mod screen;
mod shell;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use bevy_app::App;
use bevy_ecs::message::Message;
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{Res, ResMut};
use bevy_state::state::OnEnter;
use hamlet_os::process::pty::Size;
use hamlet_protocol::client::{
    self as protocol, AgentId, RequestId, ServerMessage,
    TerminalKind, TerminalOutput, TerminalView,
};
use tokio::sync::mpsc;
use tokio::task::AbortHandle;

use self::screen::{LAST, Screen, lock};
use self::shell::{Key, Launch, start};
use crate::calls::place::Workplace;
use crate::clients::Connections;
use crate::clients::received::FromClient;
use crate::closing::Serving;
use crate::core::Core;
use crate::follow_up::InputEffects;
use crate::server::{AddHandler, InputMessage, InputSender};
use crate::timers::AddTimer;

/// Messages a connection's queue keeps free before a terminal's
/// output to it is skipped, to be caught up with later.
const ROOM: usize = 64;
/// How long a terminal's watcher whose queue was full waits before
/// it is drawn again.
const CATCH_UP: Duration = Duration::from_millis(100);

/// What the terminals print, taken in as inputs; a watcher that fell
/// behind drawn again on a timer; the chief's requests; and every
/// terminal kept as the server stops.
pub(crate) fn plugin(app: &mut App) {
    app.add_message::<TerminalsPrinted>()
        .add_handler::<TerminalsPrinted, _>(take_in_printed)
        .add_handler::<FromClient<protocol::Terminal>, _>(
            chief::answer_request,
        )
        .add_timer(catch_up_due, catch_up)
        .add_systems(OnEnter(Serving::Stopping), keep_all);
}

/// The terminals printed: what they printed waits in their queue.
#[derive(Message)]
pub(crate) struct TerminalsPrinted;

/// What the terminals' tasks printed, until an input takes it in.
type Printing = Arc<Mutex<Vec<Printed>>>;

/// The server's terminals.
#[derive(Resource)]
pub(crate) struct Terminals {
    open: BTreeMap<String, Terminal>,
    printing: Printing,
    /// Where each terminal is kept between starts.
    folder: PathBuf,
    /// The number the next terminal's name takes.
    next: u64,
    /// When the watchers that fell behind are drawn again; none
    /// while none is behind.
    catch_up_at: Option<Instant>,
}

struct Terminal {
    view: TerminalView,
    screen: Arc<Mutex<Screen>>,
    /// Where typing goes. None when it takes none.
    keys: Option<mpsc::UnboundedSender<Key>>,
    /// The connections that watch it.
    watchers: BTreeMap<u64, Watcher>,
    tasks: Vec<AbortHandle>,
}

#[derive(Clone, Copy)]
struct Watcher {
    /// The last output sent.
    seq: u64,
    /// Output was skipped; the next it is sent is a redraw.
    behind: bool,
}

/// Something a terminal's tasks report to the server.
pub(crate) enum Printed {
    Output {
        id: String,
        seq: u64,
        bytes: Vec<u8>,
    },
    Ended {
        id: String,
        code: i64,
    },
}

/// Where a terminal's output goes: its screen, then the server.
#[derive(Clone)]
pub(crate) struct Feed {
    id: String,
    screen: Arc<Mutex<Screen>>,
    printing: Printing,
    inputs: InputSender,
}

impl Feed {
    pub(crate) fn print(&self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let mut screen = lock(&self.screen);
        screen.take(bytes);
        // Queued under the lock, so a watch taken under it sees each
        // output either in its redraw or after it, never both.
        self.report(Printed::Output {
            id: self.id.clone(),
            seq: screen.seq,
            bytes: bytes.to_vec(),
        });
    }

    /// Queues `printed`; the first into an empty queue wakes the
    /// runner to take the queue in.
    fn report(&self, printed: Printed) {
        let mut queue = self
            .printing
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let woken = queue.is_empty();
        queue.push(printed);
        if woken {
            // The runner is gone only as the server stops.
            let _ = self.inputs.send(TerminalsPrinted);
        }
    }

    /// Notes `command` as the last its terminal ran.
    pub(crate) fn command(&self, command: &str) {
        let command = command.trim();
        if !command.is_empty() {
            let mut screen = lock(&self.screen);
            screen.last = Some(command.chars().take(LAST).collect());
            screen.running = true;
        }
    }

    /// Prints the end of a command that ran, on a line of its own.
    pub(crate) fn ended_run(&self, code: Option<i64>) {
        lock(&self.screen).running = false;
        let fresh =
            lock(&self.screen).parser.screen().cursor_position().1
                == 0;
        let words = match code {
            Some(0) => "\x1b[2m[done]\x1b[0m".to_owned(),
            Some(code) => {
                format!("\x1b[31m[exit code {code}]\x1b[0m")
            }
            None => "\x1b[33m[stopped]\x1b[0m".to_owned(),
        };
        let start = if fresh { "" } else { "\n" };
        self.print_lines(&format!("{start}{words}\n\n"));
    }

    /// Prints `text` from a command's output, whose lines end with a
    /// bare line feed.
    pub(crate) fn print_lines(&self, text: &str) {
        self.print(text.replace('\n', "\r\n").as_bytes());
    }
}

impl Terminals {
    /// The terminals kept in `folder`.
    pub(crate) fn new(folder: PathBuf) -> Self {
        Self {
            open: BTreeMap::new(),
            printing: Printing::default(),
            folder,
            next: 1,
            catch_up_at: None,
        }
    }

    /// Whether a watcher fell behind and waits to be caught up.
    fn behind(&self) -> bool {
        self.open
            .values()
            .any(|t| t.watchers.values().any(|w| w.behind))
    }

    pub(crate) fn views(&self) -> Vec<TerminalView> {
        self.open.values().map(|t| t.view.clone()).collect()
    }

    /// Types `bytes` into terminal `id`. False when it takes none.
    pub(crate) fn type_in(&self, id: &str, bytes: Vec<u8>) -> bool {
        let Some(terminal) = self.open.get(id) else {
            return false;
        };
        let Some(keys) = &terminal.keys else {
            return false;
        };
        lock(&terminal.screen).typed(&bytes);
        keys.send(Key::Bytes(bytes)).is_ok()
    }

    /// Names terminal `id` `name`. False when there is no such
    /// terminal.
    pub(crate) fn rename(&mut self, id: &str, name: &str) -> bool {
        let Some(terminal) = self.open.get_mut(id) else {
            return false;
        };
        terminal.view.name = name.to_owned();
        terminal.view.named = true;
        self.keep(id);
        true
    }

    /// Ends terminal `id`'s processes and forgets it.
    pub(crate) fn close(&mut self, id: &str) -> bool {
        let Some(terminal) = self.open.remove(id) else {
            return false;
        };
        for task in terminal.tasks {
            task.abort();
        }
        let _ = std::fs::remove_file(self.file(id));
        true
    }

    /// Closes the terminals whose agent or hamlet is gone.
    pub(crate) fn prune(
        &mut self,
        agents: &[AgentId],
        hamlets: &[String],
    ) -> bool {
        let gone = self
            .open
            .iter()
            .filter(|(_, t)| {
                t.view
                    .hamlet
                    .as_ref()
                    .is_some_and(|h| !hamlets.contains(h))
                    || t.view
                        .agent
                        .is_some_and(|a| !agents.contains(&a))
            })
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for id in &gone {
            self.close(id);
        }
        !gone.is_empty()
    }

    /// The live commands terminal of `agent`, of `hamlet`, made when
    /// it has none. `address` names it; it prints to `inputs`.
    pub(crate) fn commands(
        &mut self,
        agent: AgentId,
        hamlet: Option<String>,
        address: &str,
        cwd: &str,
        inputs: &InputSender,
    ) -> Feed {
        let found = self.open.iter().find(|(_, t)| {
            t.view.kind == TerminalKind::Commands
                && t.view.agent == Some(agent)
        });
        let id = match found {
            Some((id, _)) => id.clone(),
            None => {
                let view = TerminalView {
                    agent: Some(agent),
                    cols: 120,
                    cwd: cwd.into(),
                    exit: None,
                    hamlet,
                    id: new_id(),
                    kind: TerminalKind::Commands,
                    last: None,
                    name: format!("{address}'s commands"),
                    named: false,
                    rows: 30,
                    running: false,
                    title: String::new(),
                };
                let id = view.id.clone();
                self.insert(view, None, Vec::new(), inputs);
                id
            }
        };
        self.feed(&id, inputs)
            .expect("the commands terminal was just found")
    }

    fn feed(&self, id: &str, inputs: &InputSender) -> Option<Feed> {
        self.open.get(id).map(|t| Feed {
            id: id.into(),
            screen: t.screen.clone(),
            printing: self.printing.clone(),
            inputs: inputs.clone(),
        })
    }

    /// Opens `view`, its shell started by `launch` when it has one,
    /// below the output `replay` redraws; it prints to `inputs`.
    fn insert(
        &mut self,
        view: TerminalView,
        launch: Option<Launch>,
        replay: Vec<u8>,
        inputs: &InputSender,
    ) -> String {
        let id = view.id.clone();
        let size = Size {
            rows: view.rows,
            cols: view.cols,
        };
        let screen = Arc::new(Mutex::new(Screen::new(size)));
        lock(&screen).ended = view.exit;
        lock(&screen).last.clone_from(&view.last);
        lock(&screen).restore(&replay);
        self.open.insert(
            id.clone(),
            Terminal {
                view,
                screen,
                keys: None,
                watchers: BTreeMap::new(),
                tasks: Vec::new(),
            },
        );
        let feed = self.feed(&id, inputs).expect("just inserted");
        if let Some(launch) = launch {
            let (keys, tasks) = start(launch, size, feed);
            let terminal =
                self.open.get_mut(&id).expect("just inserted");
            terminal.keys = Some(keys);
            terminal.tasks = tasks;
        }
        self.keep(&id);
        id
    }

    /// What `printed` means for the watchers, as messages to them;
    /// and whether the state changed.
    pub(crate) fn printed(
        &mut self,
        printed: Vec<Printed>,
        room: impl Fn(u64) -> usize,
    ) -> (Vec<(u64, ServerMessage)>, bool) {
        let mut changed = false;
        let mut touched =
            BTreeMap::<String, Vec<(u64, Vec<u8>)>>::new();
        for event in printed {
            match event {
                Printed::Output { id, seq, bytes } => {
                    touched.entry(id).or_default().push((seq, bytes));
                }
                Printed::Ended { id, code } => {
                    if let Some(terminal) = self.open.get_mut(&id) {
                        terminal.view.exit = Some(code);
                        terminal.keys = None;
                        lock(&terminal.screen).ended = Some(code);
                        changed = true;
                        self.keep(&id);
                    }
                }
            }
        }
        let mut out = Vec::new();
        for (id, prints) in touched {
            let Some(terminal) = self.open.get_mut(&id) else {
                continue;
            };
            let screen = lock(&terminal.screen);
            if let Some(title) = &screen.parser.callbacks().0
                && *title != terminal.view.title
            {
                terminal.view.title = title.clone();
                changed = true;
            }
            if screen.running != terminal.view.running {
                terminal.view.running = screen.running;
                changed = true;
            }
            if screen.last.is_some()
                && screen.last != terminal.view.last
            {
                terminal.view.last.clone_from(&screen.last);
                changed = true;
            }
            for (connection, watcher) in &mut terminal.watchers {
                if watcher.behind {
                    continue;
                }
                if room(*connection) < ROOM {
                    watcher.behind = true;
                    continue;
                }
                let data = prints
                    .iter()
                    .filter(|(seq, _)| *seq > watcher.seq)
                    .flat_map(|(_, bytes)| bytes.iter().copied())
                    .collect::<Vec<_>>();
                if data.is_empty() {
                    continue;
                }
                // Prints still on their way carry later numbers.
                watcher.seq =
                    prints.last().map_or(watcher.seq, |p| p.0);
                out.push((
                    *connection,
                    output(&id, data, false, None),
                ));
            }
        }
        if self.catch_up_at.is_none() && self.behind() {
            self.catch_up_at = Some(Instant::now() + CATCH_UP);
        }
        (out, changed)
    }

    /// Redraws terminals for the watchers that fell behind and have
    /// room again.
    pub(crate) fn catch_up(
        &mut self,
        room: impl Fn(u64) -> usize,
    ) -> Vec<(u64, ServerMessage)> {
        let mut out = Vec::new();
        for (id, terminal) in &mut self.open {
            for (connection, watcher) in &mut terminal.watchers {
                if watcher.behind && room(*connection) >= ROOM * 2 {
                    let screen = lock(&terminal.screen);
                    watcher.behind = false;
                    watcher.seq = screen.seq;
                    out.push((
                        *connection,
                        output(id, screen.redraw(), true, None),
                    ));
                }
            }
        }
        self.catch_up_at =
            self.behind().then(|| Instant::now() + CATCH_UP);
        out
    }

    /// Forgets connection `connection`'s watches.
    pub(crate) fn gone(&mut self, connection: u64) {
        for terminal in self.open.values_mut() {
            terminal.watchers.remove(&connection);
        }
    }

    fn watch(
        &mut self,
        connection: u64,
        id: &str,
        to: Option<&RequestId>,
    ) -> Option<ServerMessage> {
        let terminal = self.open.get_mut(id)?;
        let screen = lock(&terminal.screen);
        terminal.watchers.insert(
            connection,
            Watcher {
                seq: screen.seq,
                behind: false,
            },
        );
        Some(output(id, screen.redraw(), true, to))
    }
}

/// Sends what the terminals printed to the connections that watch
/// them, and publishes a change of a title, a last command or an
/// end.
fn take_in_printed(
    mut woken: InputMessage<TerminalsPrinted>,
    mut terminals: ResMut<Terminals>,
    mut connections: Connections,
    mut effects: ResMut<InputEffects>,
) {
    if woken.take().is_none() {
        return;
    }
    let printed = std::mem::take(
        &mut *terminals
            .printing
            .lock()
            .unwrap_or_else(PoisonError::into_inner),
    );
    let (out, changed) =
        terminals.printed(printed, |client| connections.room(client));
    for (client, message) in out {
        connections.send(client, message);
    }
    if changed {
        effects.state_changed_outside_change = true;
    }
}

/// When the watchers that fell behind are drawn again.
fn catch_up_due(terminals: Res<Terminals>) -> Option<Instant> {
    terminals.catch_up_at
}

/// Draws the terminals again for the watchers that fell behind and
/// have room again.
fn catch_up(
    mut terminals: ResMut<Terminals>,
    mut connections: Connections,
) -> anyhow::Result<()> {
    let caught =
        terminals.catch_up(|client| connections.room(client));
    for (client, message) in caught {
        connections.send(client, message);
    }
    Ok(())
}

/// Keeps every terminal where the next start finds it.
fn keep_all(terminals: Res<Terminals>) {
    terminals.keep_all();
}

impl Core<'_, '_> {
    /// Closes the terminals whose agent ended, a worker's when its
    /// task did, or whose hamlet is gone.
    pub(crate) fn prune_terminals(&mut self) -> anyhow::Result<()> {
        let db = self.realm.store().connection();
        let agents = crate::society::agents::living(db)?
            .into_iter()
            .map(|agent| Ok(AgentId(u64::try_from(agent.0)?)))
            .collect::<anyhow::Result<Vec<_>>>()?;
        let hamlets = crate::society::hamlets::all(db)?
            .into_iter()
            .filter(crate::society::hamlets::Hamlet::is_living)
            .map(|hamlet| hamlet.id.0)
            .collect::<Vec<_>>();
        self.terminals.prune(&agents, &hamlets);
        Ok(())
    }

    /// Where `agent`'s shell call of `command`, working at `at`,
    /// shows the command and its output as they come: the agent's
    /// commands terminal.
    pub(crate) fn commands_terminal(
        &mut self,
        agent: hamlet_store::ids::AgentId,
        at: &Workplace,
        command: &str,
    ) -> anyhow::Result<Feed> {
        let db = self.realm.store().connection();
        let address =
            crate::society::addresses::address_of(db, agent)?;
        let cwd = at.place.folder.as_ref().unwrap_or(&at.place.lodge);
        let feed = self.terminals.commands(
            AgentId(u64::try_from(agent.0)?),
            at.hamlet.as_ref().map(|hamlet| hamlet.0.clone()),
            &address,
            &hamlet_os::fs::path::text(cwd),
            &self.inputs,
        );
        let command = self.credentials.scrub_text(command);
        feed.command(&command);
        feed.print_lines(&format!("\x1b[1;36m$\x1b[0m {command}\n"));
        Ok(feed)
    }
}

impl Drop for Terminals {
    fn drop(&mut self) {
        for terminal in self.open.values() {
            for task in &terminal.tasks {
                task.abort();
            }
        }
    }
}

fn output(
    id: &str,
    data: Vec<u8>,
    reset: bool,
    to: Option<&RequestId>,
) -> ServerMessage {
    ServerMessage::Terminal(TerminalOutput {
        data: STANDARD.encode(data),
        reset,
        terminal: id.into(),
        to: to.cloned(),
    })
}

fn new_id() -> String {
    let id = uuid::Uuid::new_v4().simple().to_string();
    format!("t-{}", &id[..12])
}
