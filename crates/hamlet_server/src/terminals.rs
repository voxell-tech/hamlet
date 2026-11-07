//! Terminals: shells on pseudo-terminals that the chief and agents
//! type into, and the commands of each agent's shell tool, kept for
//! the windows that watch them and across restarts.
//!
//! A terminal of a hamlet runs under the sandbox, with its agent's
//! reach, or its mayor's when the chief opened it. An unbounded
//! terminal is the chief's own shell, with the server account's
//! reach.

pub(crate) mod chief;
pub(crate) mod kept;
mod screen;
mod shell;
mod tool;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use hamlet_os::process::pty::Size;
use hamlet_protocol::client::{
    AgentRef, RequestId, ServerMessage, TerminalKind, TerminalOutput,
    TerminalView,
};
use tokio::sync::mpsc;
use tokio::task::AbortHandle;

use self::screen::{LAST, Screen, lock};
use self::shell::{Key, Launch, start};
pub(crate) use self::tool::{Tool, call, command, specs};

/// Messages a connection's queue keeps free before a terminal's
/// output to it is skipped, to be caught up with later.
const ROOM: usize = 64;

/// The server's terminals.
pub(crate) struct Terminals {
    open: BTreeMap<String, Terminal>,
    sender: mpsc::UnboundedSender<Printed>,
    receiver: mpsc::UnboundedReceiver<Printed>,
    /// Where each terminal is kept between starts.
    folder: PathBuf,
    /// The number the next terminal's name takes.
    next: u64,
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

/// Something a terminal's tasks report to the serve loop.
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

/// Where a terminal's output goes: its screen, then the serve loop.
#[derive(Clone)]
pub(crate) struct Feed {
    id: String,
    screen: Arc<Mutex<Screen>>,
    sender: mpsc::UnboundedSender<Printed>,
}

impl Feed {
    pub(crate) fn print(&self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let mut screen = lock(&self.screen);
        screen.take(bytes);
        // Sent under the lock, so a watch taken under it sees each
        // output either in its redraw or after it, never both.
        let _ = self.sender.send(Printed::Output {
            id: self.id.clone(),
            seq: screen.seq,
            bytes: bytes.to_vec(),
        });
    }

    /// Notes `command` as the last its terminal ran.
    pub(crate) fn command(&self, command: &str) {
        let command = command.trim();
        if !command.is_empty() {
            lock(&self.screen).last =
                Some(command.chars().take(LAST).collect());
        }
    }

    /// Prints the end of a command that ran, on a line of its own.
    pub(crate) fn ended_run(&self, code: Option<i64>) {
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
    pub(crate) fn new(home: &Path) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        Self {
            open: BTreeMap::new(),
            sender,
            receiver,
            folder: home.join("terminals"),
            next: 1,
        }
    }

    /// What a terminal's tasks report next.
    pub(crate) async fn next(&mut self) -> Option<Printed> {
        self.receiver.recv().await
    }

    /// What else they reported, without waiting.
    pub(crate) fn more(&mut self) -> Option<Printed> {
        self.receiver.try_recv().ok()
    }

    pub(crate) fn views(&self) -> Vec<TerminalView> {
        self.open.values().map(|t| t.view.clone()).collect()
    }

    /// The terminal of `hamlet` with the id or the name `named`, and
    /// its screen.
    pub(crate) fn of_hamlet(
        &self,
        named: &str,
        hamlet: &str,
    ) -> Option<(TerminalView, Arc<Mutex<Screen>>)> {
        let of_hamlet =
            |t: &&Terminal| t.view.hamlet.as_deref() == Some(hamlet);
        self.open
            .get(named)
            .filter(of_hamlet)
            .or_else(|| {
                self.open
                    .values()
                    .filter(of_hamlet)
                    .rfind(|t| t.view.name == named)
            })
            .map(|t| (t.view.clone(), t.screen.clone()))
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
        agents: &[AgentRef],
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
                        .as_ref()
                        .is_some_and(|a| !agents.contains(a))
            })
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for id in &gone {
            self.close(id);
        }
        !gone.is_empty()
    }

    /// The live commands terminal of `agent`, made when it has none.
    pub(crate) fn commands(
        &mut self,
        agent: &AgentRef,
        name: &str,
        cwd: &str,
    ) -> Feed {
        let found = self.open.iter().find(|(_, t)| {
            t.view.kind == TerminalKind::Commands
                && t.view.agent.as_ref() == Some(agent)
        });
        let id = match found {
            Some((id, _)) => id.clone(),
            None => {
                let view = TerminalView {
                    agent: Some(agent.clone()),
                    cols: 120,
                    cwd: cwd.into(),
                    exit: None,
                    hamlet: Some(agent.hamlet.clone()),
                    id: new_id(),
                    kind: TerminalKind::Commands,
                    last: None,
                    name: format!("{name}'s commands"),
                    named: false,
                    rows: 30,
                    sandboxed: true,
                    title: String::new(),
                };
                let id = view.id.clone();
                self.insert(view, None, Vec::new());
                id
            }
        };
        self.feed(&id)
            .expect("the commands terminal was just found")
    }

    fn feed(&self, id: &str) -> Option<Feed> {
        self.open.get(id).map(|t| Feed {
            id: id.into(),
            screen: t.screen.clone(),
            sender: self.sender.clone(),
        })
    }

    fn insert(
        &mut self,
        view: TerminalView,
        launch: Option<Launch>,
        replay: Vec<u8>,
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
        let feed = self.feed(&id).expect("just inserted");
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
