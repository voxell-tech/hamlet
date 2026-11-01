//! How a terminal's shell starts, unbounded or under the sandbox
//! with an agent's reach, and the tasks that read and type into it.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;
use hamlet_os::process::pty::{self, Size};
use hamlet_protocol::client::{AgentRef, TerminalKind, TerminalView};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio::task::AbortHandle;

use super::screen::lock;
use super::{Feed, Printed, new_id};
use crate::protocol::{Cause, Refusable, Refusal};
use crate::record;
use crate::sandbox::Env;
use crate::service::Service;

pub(super) enum Key {
    Bytes(Vec<u8>),
    Resize(Size),
}

/// How a shell is started.
pub(super) struct Launch {
    argv: Vec<String>,
    env: BTreeMap<String, String>,
    cwd: String,
    /// The sandboxed environment, kept while the shell runs.
    sandbox: Option<Env>,
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
        sandbox: None,
    }
}

/// A shell with `agent`'s reach, under the sandbox.
pub(super) fn sandboxed(
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
pub(super) fn reach_of(
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
pub(super) fn open(
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
