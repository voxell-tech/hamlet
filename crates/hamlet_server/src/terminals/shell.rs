//! How the chief's shell starts, and the tasks that read and type
//! into it.

use std::collections::BTreeMap;
use std::path::Path;

use hamlet_os::process::pty::{self, Size};
use hamlet_protocol::client::{TerminalKind, TerminalView};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio::task::AbortHandle;

use super::screen::lock;
use super::{Feed, Printed, new_id};
use crate::core::Core;

pub(super) enum Key {
    Bytes(Vec<u8>),
    Resize(Size),
}

/// How a shell is started.
pub(super) struct Launch {
    argv: Vec<String>,
    env: BTreeMap<String, String>,
    cwd: String,
}

/// Starts `launch` on a terminal of `size`, printing to `feed`: one
/// task reads and waits for the shell, another types into it.
pub(super) fn start(
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
pub(super) fn unbounded() -> Launch {
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
    }
}

/// Opens the chief's shell at `size`. Its id.
pub(super) fn open(core: &mut Core, size: Size) -> String {
    let launch = unbounded();
    let shell = Path::new(&launch.argv[0])
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("shell")
        .to_owned();
    let number = core.terminals.next;
    core.terminals.next += 1;
    let view = TerminalView {
        agent: None,
        cols: size.cols,
        cwd: launch.cwd.clone(),
        exit: None,
        hamlet: None,
        id: new_id(),
        kind: TerminalKind::Shell,
        last: None,
        named: false,
        name: format!("{shell} {number}"),
        rows: size.rows,
        running: false,
        title: String::new(),
    };
    core.terminals.insert(view, Some(launch), Vec::new())
}

/// How a kept terminal of `view` starts again, or None when it
/// cannot: one an agent or a hamlet held belongs to the days agents
/// had shells.
pub(super) fn relaunch(view: &TerminalView) -> Option<Launch> {
    (view.agent.is_none() && view.hamlet.is_none()).then(unbounded)
}
