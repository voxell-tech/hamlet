//! Terminals: shells on pseudo-terminals that the chief and agents
//! type into, and the runs of each agent's shell tool, kept for the
//! windows that watch them and across restarts.
//!
//! A terminal of a hamlet runs under the sandbox, with its agent's
//! reach, or its mayor's when the chief opened it. An unbounded
//! terminal is the chief's own shell, with the server account's
//! reach.

mod tool;

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use anyhow::Result;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use hamlet_os::process::pty::{self, Size};
use hamlet_protocol::client::{
    AgentRef, Replayed, RequestId, ServerMessage, Terminal as Asked,
    TerminalKind, TerminalOutput, TerminalView, replayed, size_mark,
};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio::task::AbortHandle;

pub(crate) use self::tool::{Tool, call, command, specs};
use crate::protocol::{Cause, Refusable, Refusal, reply};
use crate::record;
use crate::sandbox::Env;
use crate::service::Service;

/// Bytes of output kept to redraw a terminal for a window that
/// begins watching it, and across restarts.
const REPLAY: usize = 512 * 1024;

/// Lines an agent's read of a terminal can reach above its screen.
const SCROLLBACK: usize = 2000;
/// The most characters of a last command kept.
const LAST: usize = 200;

/// Messages a connection's queue keeps free before a terminal's
/// output to it is skipped, to be caught up with later.
const ROOM: usize = 64;

/// Said in a shell that a restart started again.
const RESTARTED: &str = "\r\n\x1b[2m[The server restarted, so this \
                         is a new shell.]\x1b[0m\r\n";

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

/// What a terminal printed, and what a window needs to draw it again.
pub(crate) struct Screen {
    parser: vt100::Parser<Titled>,
    /// What it printed, with a size mark where it was resized.
    replay: VecDeque<u8>,
    /// Its size where `replay` starts.
    start: Size,
    /// Where the line being typed starts: the cursor, as a row
    /// counted from the top of the scrollback and a column, when
    /// typing began after a prompt.
    typing: Option<(usize, u16)>,
    /// Where a line entered starts, read once the shell has echoed
    /// it.
    entered: Option<(usize, u16)>,
    /// The last command typed into it or run in it.
    last: Option<String>,
    seq: u64,
    ended: Option<i64>,
}

#[derive(Default)]
struct Titled(Option<String>);

impl vt100::Callbacks for Titled {
    fn set_window_title(
        &mut self,
        _: &mut vt100::Screen,
        title: &[u8],
    ) {
        self.0 = Some(String::from_utf8_lossy(title).into_owned());
    }
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

enum Key {
    Bytes(Vec<u8>),
    Resize(Size),
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

/// `n` cells, at least one.
fn cells(n: u16) -> std::num::NonZeroU16 {
    std::num::NonZeroU16::new(n).unwrap_or(std::num::NonZeroU16::MIN)
}

fn lock(screen: &Mutex<Screen>) -> std::sync::MutexGuard<'_, Screen> {
    screen.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Screen {
    fn new(size: Size) -> Self {
        Self {
            parser: vt100::Parser::new_with_callbacks(
                cells(size.rows),
                cells(size.cols),
                SCROLLBACK,
                Titled::default(),
            ),
            replay: VecDeque::new(),
            start: size,
            typing: None,
            entered: None,
            last: None,
            seq: 0,
            ended: None,
        }
    }

    /// Takes the bytes a redraw of it holds, at their sizes.
    fn restore(&mut self, bytes: &[u8]) {
        for part in replayed(bytes) {
            match part {
                Replayed::Printed(bytes) => self.take(bytes),
                Replayed::Sized { cols, rows } => {
                    self.resize(Size { rows, cols });
                }
            }
        }
    }

    fn take(&mut self, bytes: &[u8]) {
        let mut rest = bytes;
        // The line entered is read at the echo of its Enter, before
        // what the command prints scrolls it away.
        if self.entered.is_some()
            && let Some(at) = rest.iter().position(|b| *b == b'\n')
        {
            self.parser.process(&rest[..=at]);
            self.read_entered();
            rest = &rest[at + 1..];
        }
        self.parser.process(rest);
        self.replay.extend(bytes);
        if self.replay.len() > REPLAY {
            let over = self.replay.len() - REPLAY;
            // Starts the kept bytes at a line, not inside a sequence.
            let cut = self
                .replay
                .range(over..)
                .position(|byte| *byte == b'\n')
                .map_or(self.replay.len(), |at| over + at + 1);
            let dropped =
                self.replay.drain(..cut).collect::<Vec<_>>();
            let sized = replayed(&dropped)
                .into_iter()
                .rev()
                .find_map(|part| match part {
                    Replayed::Sized { cols, rows } => {
                        Some(Size { rows, cols })
                    }
                    Replayed::Printed(_) => None,
                });
            if let Some(size) = sized {
                self.start = size;
            }
        }
        self.seq += 1;
    }

    /// The bytes that draw it again from a blank screen, each at the
    /// size it was printed at.
    fn replay(&self) -> Vec<u8> {
        let mut bytes = size_mark(self.start.cols, self.start.rows);
        bytes.extend(self.replay.iter());
        bytes
    }

    /// The cursor, its row counted from the top of the scrollback.
    fn cursor(&self) -> (usize, u16) {
        let screen = self.parser.screen();
        let (row, col) = screen.cursor_position();
        (screen.scrollback_len() + usize::from(row), col)
    }

    /// Notes what `bytes` typed means for the last command: where the
    /// line starts, as typing begins after a prompt, and the line
    /// entered with Enter. A full-screen program's keys are not
    /// commands.
    fn typed(&mut self, bytes: &[u8]) {
        if self.parser.screen().alternate_screen() {
            self.typing = None;
            return;
        }
        // Ctrl+C and Ctrl+L leave the line where it was.
        if bytes.iter().any(|b| matches!(b, 0x03 | 0x0c)) {
            self.typing = None;
            return;
        }
        let cursor = self.cursor();
        let start = *self.typing.get_or_insert(cursor);
        if bytes.contains(&b'\r') {
            self.entered = Some(start);
            self.typing = None;
        }
    }

    /// Reads the line entered once the cursor has left it.
    fn read_entered(&mut self) {
        let Some((row, col)) = self.entered else {
            return;
        };
        if self.cursor().0 <= row {
            return;
        }
        self.entered = None;
        let line = self.line_from(row, col);
        if !line.is_empty() {
            self.last = Some(line);
        }
    }

    /// The line that starts at `row` and `col`, its wrapped rows
    /// joined, without what a prompt shows at its right.
    fn line_from(&mut self, row: usize, col: u16) -> String {
        let screen = self.parser.screen_mut();
        let back = screen.scrollback_len();
        let (rows, cols) = screen.size();
        let (rows, cols) = (rows.get(), cols.get());
        let mut at = if row >= back {
            u16::try_from(row - back).unwrap_or(rows)
        } else {
            screen.set_scrollback(back - row);
            0
        };
        let mut text = String::new();
        let mut from = col;
        while at < rows {
            text.push_str(
                &screen.contents_between(at, from, at, cols),
            );
            if !screen.row_wrapped(at) {
                break;
            }
            at += 1;
            from = 0;
        }
        screen.set_scrollback(0);
        let text = text.split("    ").next().unwrap_or_default();
        text.trim().chars().take(LAST).collect()
    }

    fn resize(&mut self, size: Size) {
        let (rows, cols) = self.parser.screen().size();
        if (rows, cols) == (cells(size.rows), cells(size.cols)) {
            return;
        }
        // Rewrapping moves the lines being typed.
        self.typing = None;
        self.entered = None;
        hamlet_protocol::screen::resize(
            &mut self.parser,
            cells(size.rows),
            cells(size.cols),
        );
        self.replay.extend(size_mark(size.cols, size.rows));
    }

    /// The text it shows, with up to `above` lines of scrollback.
    pub(crate) fn text(&mut self, above: usize) -> String {
        let screen = self.parser.screen_mut();
        screen.set_scrollback(above);
        let shown = screen.scrollback();
        let (rows, cols) = screen.size();
        let (rows, cols) = (usize::from(rows.get()), cols.get());
        let mut lines =
            screen.rows(0, cols).take(rows).collect::<Vec<_>>();
        if shown > 0 {
            screen.set_scrollback(0);
            let now = screen.rows(0, cols).collect::<Vec<_>>();
            let skip = rows.saturating_sub(shown);
            lines.extend(now.into_iter().skip(skip));
        }
        while lines.last().is_some_and(|line| line.trim().is_empty())
        {
            lines.pop();
        }
        lines
            .iter()
            .map(|line| line.trim_end())
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub(crate) fn ended(&self) -> Option<i64> {
        self.ended
    }

    pub(crate) fn seq(&self) -> u64 {
        self.seq
    }
}

/// A terminal as it is kept between starts.
#[derive(Serialize, Deserialize)]
struct Kept {
    view: TerminalView,
    /// The bytes that redraw it, in base64.
    replay: String,
}

/// How a shell is started.
struct Launch {
    argv: Vec<String>,
    env: BTreeMap<String, String>,
    cwd: String,
    /// The sandboxed environment, kept while the shell runs.
    sandbox: Option<Env>,
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

    /// The live runs terminal of `agent`, made when it has none.
    pub(crate) fn runs(
        &mut self,
        agent: &AgentRef,
        name: &str,
        cwd: &str,
    ) -> Feed {
        let found = self.open.iter().find(|(_, t)| {
            t.view.kind == TerminalKind::Runs
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
                    kind: TerminalKind::Runs,
                    last: None,
                    name: format!("{name}'s runs"),
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
        self.feed(&id).expect("the runs terminal was just found")
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

    fn file(&self, id: &str) -> PathBuf {
        self.folder.join(format!("{id}.json"))
    }

    /// Writes terminal `id` where the next start finds it.
    fn keep(&self, id: &str) {
        let Some(terminal) = self.open.get(id) else {
            return;
        };
        let kept = Kept {
            view: terminal.view.clone(),
            replay: STANDARD.encode(lock(&terminal.screen).replay()),
        };
        let written =
            std::fs::create_dir_all(&self.folder).and_then(|()| {
                let text = serde_json::to_vec(&kept)?;
                let path = self.file(id);
                let part = path.with_extension("part");
                std::fs::write(&part, text)?;
                std::fs::rename(part, path)
            });
        if let Err(error) = written {
            crate::diagnostic::tell(
                crate::diagnostic::Secrets::Fixed,
                format_args!("could not keep a terminal: {error}"),
            );
        }
    }

    /// Writes every terminal where the next start finds it.
    pub(crate) fn keep_all(&self) {
        for id in self.open.keys() {
            self.keep(id);
        }
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
                        output(id, screen.replay(), true, None),
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
        Some(output(id, screen.replay(), true, to))
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

/// Starts `launch` on a terminal of `size`, printing to `feed`: one
/// task reads and waits for the shell, another types into it.
fn start(
    launch: Launch,
    size: Size,
    feed: Feed,
) -> (mpsc::UnboundedSender<Key>, Vec<AbortHandle>) {
    let (keys, mut typed) = mpsc::unbounded_channel::<Key>();
    let opened = pty::open(
        pty::Command {
            argv: &launch.argv,
            env: &launch.env,
            cwd: &launch.cwd,
        },
        size,
    );
    let (mut session, mut input, mut output) = match opened {
        Ok(opened) => opened,
        Err(error) => {
            feed.print_lines(&format!(
                "\n[The shell could not start: {error}]\n"
            ));
            ended(&feed, -1);
            return (keys, Vec::new());
        }
    };
    let reader = tokio::spawn(async move {
        // Kept until the shell ends: its private temp folder.
        let _sandbox = launch.sandbox;
        let mut buffer = vec![0; 64 * 1024];
        loop {
            match output.read(&mut buffer).await {
                Ok(0) | Err(_) => break,
                Ok(n) => feed.print(&buffer[..n]),
            }
        }
        let code = match session.ended().await {
            Ok(status) => exit_code(status),
            Err(_) => -1,
        };
        ended(&feed, code);
    });
    let writer = tokio::spawn(async move {
        while let Some(key) = typed.recv().await {
            match key {
                Key::Bytes(bytes) => {
                    if input.write_all(&bytes).await.is_err() {
                        break;
                    }
                }
                Key::Resize(size) => {
                    let _ = input.resize(size);
                }
            }
        }
    });
    (keys, vec![reader.abort_handle(), writer.abort_handle()])
}

fn ended(feed: &Feed, code: i64) {
    lock(&feed.screen).ended = Some(code);
    let _ = feed.sender.send(Printed::Ended {
        id: feed.id.clone(),
        code,
    });
}

fn exit_code(status: std::process::ExitStatus) -> i64 {
    #[cfg(unix)]
    if let Some(signal) =
        std::os::unix::process::ExitStatusExt::signal(&status)
    {
        return 128 + i64::from(signal);
    }
    status.code().map_or(-1, i64::from)
}

/// The environment every shell gets on top of its own.
fn terminal_env(env: &mut BTreeMap<String, String>) {
    for (name, value) in [
        ("TERM", "xterm-256color"),
        ("COLORTERM", "truecolor"),
        ("TERM_PROGRAM", "Hamlet"),
        ("BASH_SILENCE_DEPRECATION_WARNING", "1"),
    ] {
        env.insert(name.into(), value.into());
    }
}

/// The chief's own shell: the login shell, in their home folder.
fn unbounded() -> Launch {
    let shell = std::env::var("SHELL")
        .ok()
        .filter(|shell| !shell.is_empty())
        .unwrap_or_else(|| "/bin/bash".into());
    let mut env = std::env::vars()
        .filter(|(name, _)| !name.starts_with("HAMLET_"))
        .collect::<BTreeMap<_, _>>();
    terminal_env(&mut env);
    let cwd = env.get("HOME").cloned().unwrap_or_else(|| "/".into());
    Launch {
        argv: vec![shell, "-l".into()],
        env,
        cwd,
        sandbox: None,
    }
}

/// A shell with `agent`'s reach, under the sandbox.
fn sandboxed(
    service: &mut Service,
    agent: &record::AgentRef,
    net: bool,
) -> Result<Refusable<Launch>> {
    let Some(sandbox) = service.houses.sandbox() else {
        return Ok(Err(Refusal::Cannot {
            operation: "open a terminal".into(),
            cause: Cause::Remote,
        }));
    };
    let place = crate::chats::places(service, agent)?;
    let env = match crate::tools::environment(&sandbox, &place, net) {
        Ok(env) => env,
        Err(error) => {
            return Ok(Err(Refusal::Invalid(format!(
                "The sandbox could not be set up: {error}"
            ))));
        }
    };
    let mut confined = env.terminal(&[
        "bash".into(),
        "--noprofile".into(),
        "--norc".into(),
        // No history expansion, so a `!` an agent types stays one.
        "+H".into(),
        "-i".into(),
    ]);
    terminal_env(&mut confined.env);
    confined.env.insert(
        "PS1".into(),
        "\\[\\e[1;32m\\]\\W\\[\\e[0m\\] \
         \\[\\e[2m\\]sandboxed\\[\\e[0m\\] \\$ "
            .into(),
    );
    Ok(Ok(Launch {
        argv: confined.argv,
        env: confined.env,
        cwd: env.cwd().to_owned(),
        sandbox: Some(env),
    }))
}

/// The agent whose reach a terminal of `hamlet` has: `agent`, or the
/// mayor's when the chief opens it.
fn reach_of(
    record: &record::Hamlet,
    agent: Option<&AgentRef>,
) -> record::AgentRef {
    record::AgentRef {
        hamlet: record.id.clone(),
        number: agent.map_or(record.mayor, |a| a.number),
    }
}

/// Opens a terminal for `agent`, or for the chief when none, in
/// `hamlet`, or unbounded when none. Its id, or why not.
pub(crate) fn open(
    service: &mut Service,
    hamlet: Option<&str>,
    agent: Option<&AgentRef>,
    name: Option<String>,
    size: Size,
    net: bool,
) -> Result<Refusable<String>> {
    let number = service.terminals.next;
    let (launch, sandboxed, hamlet, label) = match hamlet {
        None => {
            let launch = unbounded();
            let shell = Path::new(&launch.argv[0])
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("shell")
                .to_owned();
            (launch, false, None, shell)
        }
        Some(hamlet) => {
            let record =
                match service.access(hamlet, "open a terminal")? {
                    Ok(record) => record,
                    Err(refusal) => return Ok(Err(refusal)),
                };
            let reach = reach_of(&record, agent);
            let Some(member) = record
                .agents
                .iter()
                .find(|a| a.number == reach.number)
            else {
                return Ok(Err(Refusal::NoAgent(
                    reach.number.to_string(),
                )));
            };
            let label = match agent {
                Some(_) => member.name.clone(),
                None => record.id.clone(),
            };
            let launch = match sandboxed(service, &reach, net)? {
                Ok(launch) => launch,
                Err(refusal) => return Ok(Err(refusal)),
            };
            (launch, true, Some(hamlet.to_owned()), label)
        }
    };
    service.terminals.next += 1;
    let view = TerminalView {
        agent: agent.cloned(),
        cols: size.cols,
        cwd: launch.cwd.clone(),
        exit: None,
        hamlet,
        id: new_id(),
        kind: TerminalKind::Shell,
        last: None,
        named: name.is_some(),
        name: name.unwrap_or_else(|| format!("{label} {number}")),
        rows: size.rows,
        sandboxed,
        title: String::new(),
    };
    Ok(Ok(service.terminals.insert(view, Some(launch), Vec::new())))
}

/// Opens again the terminals kept at the last stop: a new shell where
/// one ran, below what the old one printed.
pub(crate) fn restore(service: &mut Service) -> Result<()> {
    let folder = service.terminals.folder.clone();
    let Ok(entries) = std::fs::read_dir(&folder) else {
        return Ok(());
    };
    let mut kept = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|e| e == "json"))
        .filter_map(|path| {
            let text = std::fs::read(&path).ok()?;
            let kept = serde_json::from_slice::<Kept>(&text).ok();
            if kept.is_none() {
                let _ = std::fs::remove_file(&path);
            }
            kept
        })
        .collect::<Vec<_>>();
    kept.sort_by(|a, b| a.view.name.cmp(&b.view.name));
    for Kept { mut view, replay } in kept {
        let mut replay = STANDARD.decode(replay).unwrap_or_default();
        let launch = match (&view.kind, view.exit, &view.hamlet) {
            (TerminalKind::Runs, ..) | (_, Some(_), _) => None,
            (TerminalKind::Shell, None, None) => Some(unbounded()),
            (TerminalKind::Shell, None, Some(hamlet)) => {
                let record = service.home.store().record(hamlet)?;
                let live = record.filter(|record| {
                    view.agent.as_ref().is_none_or(|agent| {
                        record
                            .agents
                            .iter()
                            .any(|a| a.number == agent.number)
                    })
                });
                let Some(record) = live else {
                    let _ = std::fs::remove_file(
                        service.terminals.file(&view.id),
                    );
                    continue;
                };
                let reach = reach_of(&record, view.agent.as_ref());
                sandboxed(service, &reach, view.agent.is_none())?.ok()
            }
        };
        if launch.is_some() {
            replay.extend_from_slice(RESTARTED.as_bytes());
        } else if view.exit.is_none()
            && view.kind == TerminalKind::Shell
        {
            view.exit = Some(-1);
        }
        service.terminals.insert(view, launch, replay);
    }
    service.terminals.next = service.terminals.open.len() as u64 + 1;
    Ok(())
}

/// Answers the chief's terminal request `asked` from connection
/// `connection`.
pub(crate) fn request(
    service: &mut Service,
    connection: u64,
    asked: &Asked,
) -> Result<Vec<ServerMessage>> {
    Ok(match asked {
        Asked::Open {
            cols,
            hamlet,
            id,
            rows,
        } => {
            let size = Size {
                rows: *rows,
                cols: *cols,
            };
            match open(
                service,
                hamlet.as_deref(),
                None,
                None,
                size,
                true,
            )? {
                Ok(terminal) => service
                    .terminals
                    .watch(connection, &terminal, Some(id))
                    .into_iter()
                    .collect(),
                Err(refusal) => vec![reply(id, Err(refusal))],
            }
        }
        Asked::Input { data, terminal } => {
            service
                .terminals
                .type_in(terminal, data.as_bytes().to_vec());
            Vec::new()
        }
        Asked::Resize {
            cols,
            rows,
            terminal,
        } => {
            let size = Size {
                rows: *rows,
                cols: *cols,
            };
            if let Some(open) =
                service.terminals.open.get_mut(terminal)
            {
                open.view.rows = size.rows;
                open.view.cols = size.cols;
                lock(&open.screen).resize(size);
                if let Some(keys) = &open.keys {
                    let _ = keys.send(Key::Resize(size));
                }
            }
            Vec::new()
        }
        Asked::Watch { id, terminal } => {
            match service.terminals.watch(
                connection,
                terminal,
                Some(id),
            ) {
                Some(output) => vec![output],
                None => vec![reply(id, Err(gone(terminal)))],
            }
        }
        Asked::Unwatch { terminal } => {
            if let Some(open) =
                service.terminals.open.get_mut(terminal)
            {
                open.watchers.remove(&connection);
            }
            Vec::new()
        }
        Asked::Close { id, terminal } => {
            let result = if service.terminals.close(terminal) {
                Ok("Closed.".into())
            } else {
                Err(gone(terminal))
            };
            vec![reply(id, result)]
        }
        Asked::Rename { id, name, terminal } => {
            let name = name.trim();
            let result = if name.is_empty() {
                Err(Refusal::Invalid(
                    "A terminal needs a name.".into(),
                ))
            } else if service.terminals.rename(terminal, name) {
                Ok("Renamed.".into())
            } else {
                Err(gone(terminal))
            };
            vec![reply(id, result)]
        }
    })
}

fn gone(terminal: &str) -> Refusal {
    Refusal::Invalid(format!("There is no terminal `{terminal}`."))
}

/// What a terminal's output is for an agent, as its tools show it.
pub(crate) fn shown(
    view: &TerminalView,
    screen: &Mutex<Screen>,
    above: usize,
) -> String {
    let mut screen = lock(screen);
    let text = screen.text(above);
    let status = match screen.ended() {
        Some(code) => format!("ended with exit code {code}"),
        None => "running".into(),
    };
    format!(
        "[terminal {} \"{}\", {}x{}, {status}]\n{text}",
        view.id, view.name, view.cols, view.rows
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_agent_reads_the_screen_with_the_scrollback_it_asks_for() {
        let mut screen = Screen::new(Size { rows: 3, cols: 20 });
        for line in 1..=6 {
            screen.take(format!("line {line}\r\n").as_bytes());
        }
        assert_eq!(screen.text(0), "line 5\nline 6");
        assert_eq!(screen.text(2), "line 3\nline 4\nline 5\nline 6");
        assert_eq!(screen.text(100).lines().next(), Some("line 1"));
    }

    #[test]
    fn the_replay_keeps_its_last_bytes_from_the_start_of_a_line() {
        let mut screen = Screen::new(Size { rows: 3, cols: 20 });
        let line = format!("{}\n", "x".repeat(1023));
        for _ in 0..(REPLAY / 1024 + 10) {
            screen.take(line.as_bytes());
        }
        let replay = screen.replay();
        let mark = size_mark(20, 3);
        assert!(replay.starts_with(&mark));
        let replay = &replay[mark.len()..];
        assert!(replay.len() <= REPLAY, "{}", replay.len());
        assert!(replay.len() > REPLAY - 1024);
        assert_eq!(replay[0], b'x');
        assert_eq!(replay[replay.len() - 1], b'\n');
    }

    #[test]
    fn a_redraw_draws_each_part_at_the_size_it_was_printed_at() {
        let mut screen = Screen::new(Size { rows: 3, cols: 4 });
        screen.take(b"abcdef");
        screen.resize(Size { rows: 3, cols: 8 });
        screen.resize(Size { rows: 3, cols: 8 });
        screen.take(b"\r\ngh");
        let replay = screen.replay();
        assert_eq!(
            replayed(&replay),
            [
                Replayed::Sized { cols: 4, rows: 3 },
                Replayed::Printed(b"abcdef"),
                Replayed::Sized { cols: 8, rows: 3 },
                Replayed::Printed(b"\r\ngh"),
            ]
        );
        let mut again = Screen::new(Size { rows: 3, cols: 8 });
        again.restore(&replay);
        assert_eq!(again.text(0), "abcdef\ngh");
        assert_eq!(again.text(0), screen.text(0));
    }

    /// Types `keys` into `screen`, and has the shell echo `echo`.
    fn typed(screen: &mut Screen, keys: &str, echo: &str) {
        screen.typed(keys.as_bytes());
        screen.take(echo.as_bytes());
    }

    #[test]
    fn the_last_command_is_the_line_typed_after_the_prompt() {
        let mut screen = Screen::new(Size { rows: 4, cols: 12 });
        screen.take(b"$ ");
        typed(&mut screen, "l", "l");
        typed(&mut screen, "s -la /tmp", "s -la /tmp");
        typed(&mut screen, "\r", "\r\n");
        screen.take(b"total 0\r\n$ ");
        assert_eq!(screen.last.as_deref(), Some("ls -la /tmp"));
        // A prompt that hides what is typed shows no command.
        screen.take(b"pass: ");
        typed(&mut screen, "secret\r", "\r\n");
        assert_eq!(screen.last.as_deref(), Some("ls -la /tmp"));
        // Nor what is typed at a full-screen program.
        screen.take(b"\x1b[?1049h");
        typed(&mut screen, "q\r", "\r\n");
        assert_eq!(screen.last.as_deref(), Some("ls -la /tmp"));
    }

    #[test]
    fn the_last_command_leaves_out_what_a_prompt_shows_at_its_right()
    {
        let mut screen = Screen::new(Size { rows: 3, cols: 30 });
        screen.take(b"% \x1b[s\x1b[1;25H~/src\x1b[u");
        typed(&mut screen, "make\r", "make\r\n");
        assert_eq!(screen.last.as_deref(), Some("make"));
    }

    #[test]
    fn a_trimmed_replay_starts_at_the_size_its_first_kept_byte_had() {
        let mut screen = Screen::new(Size { rows: 3, cols: 20 });
        screen.take(b"first\n");
        screen.resize(Size { rows: 4, cols: 30 });
        let line = format!("{}\n", "x".repeat(1023));
        for _ in 0..(REPLAY / 1024 + 10) {
            screen.take(line.as_bytes());
        }
        assert!(screen.replay().starts_with(&size_mark(30, 4)));
    }
}
