//! How a terminal's shell starts, unbounded or under the sandbox
//! with an agent's reach or a hamlet's folder, and the tasks that
//! read and type into it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use hamlet_os::fs::path::text;
use hamlet_os::process::pty::{self, Size};
use hamlet_protocol::client::{
    self as protocol, TerminalKind, TerminalView,
};
use hamlet_sandbox::{Env, Policy, Sandbox, host_env};
use hamlet_store::ids::{AgentId, HamletId};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio::task::AbortHandle;

use super::screen::lock;
use super::{Feed, Printed, new_id};
use crate::core::Core;
use crate::society::{addresses, authority, hamlets};

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

/// Who a terminal is opened for, which decides where it runs.
#[derive(Clone, Debug)]
pub(super) enum Owner {
    /// The chief's own shell, with no sandbox.
    Unbounded,
    /// The chief's shell in a hamlet's folder.
    Hamlet(HamletId),
    /// An agent's shell, with its reach.
    Agent(AgentId),
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

/// A bash shell in `env`, under its sandbox.
fn sandboxed(env: Env) -> Launch {
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
    Launch {
        argv: confined.argv,
        env: confined.env,
        cwd: env.cwd().to_owned(),
        sandbox: Some(env),
    }
}

/// The environment of the chief's shell in `folder`, a hamlet's: the
/// machine to read but the realm, and the folder alone to write, so
/// no agent's lodge is written by anyone but its agent.
fn hamlet_env(
    sandbox: &Sandbox,
    folder: &Path,
    realm: &Path,
) -> Result<Env, hamlet_sandbox::SetupError> {
    let policy = Policy {
        rw: vec![folder.to_owned()],
        open: true,
        hidden: vec![realm.to_owned()],
        env: host_env(),
        net: true,
        ..Policy::default()
    };
    Env::new(&text(folder), policy, *sandbox)
}

/// How a terminal for `owner` starts, the hamlet it belongs to and
/// what its name starts with; or why it cannot open.
fn launch(
    core: &mut Core,
    owner: &Owner,
    net: bool,
) -> Result<Result<(Launch, Option<String>, String), String>> {
    let remote = core.workspace.is_some();
    let layout = core.realm.layout().clone();
    let chief = core.realm.chief();
    let db = core.realm.store().connection();
    let setup = |error: hamlet_sandbox::SetupError| {
        format!("The sandbox could not be set up: {error}")
    };
    Ok(Ok(match owner {
        Owner::Unbounded => {
            let launch = unbounded();
            let shell = Path::new(&launch.argv[0])
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("shell")
                .to_owned();
            (launch, None, shell)
        }
        _ if remote => {
            return Ok(Err("Terminals run on this machine, and \
                           the agents work in a remote workspace."
                .into()));
        }
        Owner::Hamlet(hamlet) => {
            let found = hamlets::find(db, hamlet)?
                .filter(hamlets::Hamlet::is_living);
            let Some(found) = found else {
                return Ok(Err(format!(
                    "There is no hamlet `{hamlet}`."
                )));
            };
            if !authority::is_chief_of(db, chief, hamlet)? {
                return Ok(Err(format!(
                    "The hamlet `{hamlet}` is not yours."
                )));
            }
            let Some(folder) = found.folder.map(PathBuf::from) else {
                return Ok(Err(format!(
                    "The hamlet `{hamlet}` works on no folder, so \
                     it has no terminals of the chief's."
                )));
            };
            let env = match hamlet_env(
                &core.sandbox,
                &folder,
                layout.root(),
            ) {
                Ok(env) => env,
                Err(error) => return Ok(Err(setup(error))),
            };
            (sandboxed(env), Some(hamlet.0.clone()), hamlet.0.clone())
        }
        Owner::Agent(agent) => {
            let at = crate::calls::place::of(db, &layout, *agent)?;
            let address = addresses::address_of(db, *agent)?;
            let env = match crate::work::tools::environment(
                &core.sandbox,
                &at.place,
                net,
            ) {
                Ok(env) => env,
                Err(error) => return Ok(Err(setup(error))),
            };
            (sandboxed(env), at.hamlet.map(|h| h.0), address)
        }
    }))
}

/// Opens a terminal for `owner`, named `name` or after its owner,
/// reaching the network with `net` when it is an agent's. Its id, or
/// why not.
pub(super) fn open(
    core: &mut Core,
    owner: &Owner,
    name: Option<String>,
    size: Size,
    net: bool,
) -> Result<Result<String, String>> {
    let (launch, hamlet, label) = match launch(core, owner, net)? {
        Ok(found) => found,
        Err(words) => return Ok(Err(words)),
    };
    let number = core.terminals.next;
    core.terminals.next += 1;
    let agent = match owner {
        Owner::Agent(agent) => {
            Some(protocol::AgentId(u64::try_from(agent.0)?))
        }
        _ => None,
    };
    let view = TerminalView {
        agent,
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
        sandboxed: launch.sandbox.is_some(),
        title: String::new(),
    };
    Ok(Ok(core.terminals.insert(view, Some(launch), Vec::new())))
}

/// How a kept terminal of `view` starts again, or None when it
/// cannot. An agent's has no network until a call of its asks.
pub(super) fn relaunch(
    core: &mut Core,
    view: &TerminalView,
) -> Result<Option<Launch>> {
    let owner = match (&view.agent, &view.hamlet) {
        (Some(agent), _) => {
            Owner::Agent(AgentId(i64::try_from(agent.0)?))
        }
        (None, Some(hamlet)) => {
            Owner::Hamlet(HamletId(hamlet.clone()))
        }
        (None, None) => Owner::Unbounded,
    };
    let net = view.agent.is_none();
    Ok(match launch(core, &owner, net)? {
        Ok((launch, ..)) => Some(launch),
        Err(_) => None,
    })
}
