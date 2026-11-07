//! Terminals: shells on pseudo-terminals that the chief and agents
//! type into, and the commands of each agent's shell tool, kept for
//! the windows that watch them and across restarts.
//!
//! An agent's terminal runs under the sandbox with that agent's
//! reach. One the chief opens in a hamlet runs under the sandbox
//! with the hamlet's folder alone to write. An unbounded terminal is
//! the chief's own shell, with the server account's reach.

pub(crate) mod chief;
pub(crate) mod kept;
mod screen;
mod shell;
mod tool;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use hamlet_os::process::pty::Size;
use hamlet_protocol::client::{
    AgentId, RequestId, ServerMessage, TerminalKind, TerminalOutput,
    TerminalView,
};
use tokio::sync::mpsc;
use tokio::task::AbortHandle;

use self::screen::{LAST, Screen, lock};
use self::shell::{Key, Launch, start};
pub(crate) use self::tool::{Tool, call, command, specs};
use crate::calls::place::Workplace;
use crate::core::Core;

/// Messages a connection's queue keeps free before a terminal's
/// output to it is skipped, to be caught up with later.
const ROOM: usize = 64;

/// The server's terminals.
pub(crate) struct Terminals {
    open: BTreeMap<String, Terminal>,
    sender: mpsc::UnboundedSender<Printed>,
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
    /// The terminals kept in `folder`, which report to `sender`.
    pub(crate) fn new(
        folder: PathBuf,
        sender: mpsc::UnboundedSender<Printed>,
    ) -> Self {
        Self {
            open: BTreeMap::new(),
            sender,
            folder,
            next: 1,
        }
    }

    /// Whether a watcher fell behind and waits to be caught up.
    pub(crate) fn behind(&self) -> bool {
        self.open
            .values()
            .any(|t| t.watchers.values().any(|w| w.behind))
    }

    pub(crate) fn views(&self) -> Vec<TerminalView> {
        self.open.values().map(|t| t.view.clone()).collect()
    }

    /// The terminal with the id or the name `named` that `agent`, of
    /// `hamlet`, may use, and its screen: its own, or one the chief
    /// opened in its hamlet, which writes only the hamlet's folder.
    /// Another agent's would reach that agent's lodge.
    pub(crate) fn usable(
        &self,
        named: &str,
        agent: AgentId,
        hamlet: Option<&str>,
    ) -> Option<(TerminalView, Arc<Mutex<Screen>>)> {
        let usable = |t: &&Terminal| match t.view.agent {
            Some(owner) => owner == agent,
            None => {
                hamlet.is_some() && t.view.hamlet.as_deref() == hamlet
            }
        };
        self.open
            .get(named)
            .filter(usable)
            .or_else(|| {
                self.open
                    .values()
                    .filter(usable)
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
    /// it has none. `address` names it.
    pub(crate) fn commands(
        &mut self,
        agent: AgentId,
        hamlet: Option<String>,
        address: &str,
        cwd: &str,
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

impl Core {
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

    /// Sends what the terminals printed to the connections that watch
    /// them, and publishes a change of a title, a last command or an
    /// end.
    pub(crate) async fn printed(
        &mut self,
        printed: Vec<Printed>,
    ) -> anyhow::Result<()> {
        let clients = &self.clients.list;
        let (out, changed) = self.terminals.printed(printed, |id| {
            clients.get(&id).map_or(0, |c| c.send.capacity())
        });
        for (client, message) in out {
            self.answer(client, message)?;
        }
        if changed {
            self.publish().await?;
        }
        Ok(())
    }

    /// Draws the terminals again for the watchers that fell behind
    /// and have room again.
    pub(crate) fn catch_up(&mut self) -> anyhow::Result<()> {
        let clients = &self.clients.list;
        let caught = self.terminals.catch_up(|id| {
            clients.get(&id).map_or(0, |c| c.send.capacity())
        });
        for (client, message) in caught {
            self.answer(client, message)?;
        }
        Ok(())
    }

    /// Where `agent`'s shell call of `command`, working at `at`,
    /// shows the command and its output as they come: the agent's
    /// commands terminal.
    pub(crate) fn commands(
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
