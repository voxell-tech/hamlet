//! Hamlet's commands and process lifetimes.

#![expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "a terminal program: printing is its job"
)]

mod chat;
mod client;
mod process;
mod terminal;

use anyhow::{Result, bail};

use crate::process::{Action, Isolation};

const USAGE: &str = "usage: hamlet [--insecure]
       hamlet stop
       hamlet status
       hamlet server [--port <n>] [--faux <script.json>] [--insecure]
       hamlet dev [--port <n>] [--faux <script.json>] [--home <dir>] \
                     [--insecure]
       hamlet window
       hamlet chat [--to <agent|name@hamlet|mayor@hamlet>]
       hamlet edit <charter|character|notes|limits|settings> \
                     [--hamlet <id>]
       hamlet login <provider> [--key]
       hamlet models [try [model] | refresh]

Bare hamlet starts or reuses a background server and opens a window.
Closing a window stops no agent. hamlet stop stops the local server.
dev builds the checkout's server and window and serves a scratch \
                     home; Ctrl-C stops both.
server serves in the foreground; Ctrl-C stops it.
--insecure, on Windows only, runs agents' commands with no sandbox.

env: HAMLET_HOME (~/.hamlet), HAMLET_SERVER and HAMLET_TOKEN (remote \
                     clients),
     HAMLET_APP (window executable), HAMLET_VIEW (window settings),
     HAMLET_SERVER_BIN (server executable), HAMLET_NODE (Node \
                     executable),
     HAMLET_PI_HARNESS (Pi harness entry).
Use Node from harnesses/pi/.nvmrc and npm ci --prefix harnesses/pi \
                     before starting a server from source.";

enum Command {
    Help,
    Service(Action),
    Server {
        port: u16,
        faux: Option<String>,
        isolation: Isolation,
    },
    Dev {
        port: u16,
        faux: Option<String>,
        home: Option<String>,
        isolation: Isolation,
    },
    Window,
    Chat(String),
    Terminal(terminal::Command),
}

fn parse(args: &[String]) -> Result<Command> {
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        return Ok(Command::Help);
    }
    let Some(name) = args.first() else {
        return Ok(Command::Service(Action::Start(
            Isolation::Sandboxed,
        )));
    };
    let rest = &args[1..];
    let unknown = |arg: &str| {
        anyhow::anyhow!("unknown argument `{arg}`\n\n{USAGE}")
    };
    match name.as_str() {
        "--insecure" => {
            if let Some(arg) = rest.first() {
                return Err(unknown(arg));
            }
            Ok(Command::Service(Action::Start(insecure()?)))
        }
        "stop" | "status" | "window" => {
            if let Some(arg) = rest.first() {
                return Err(unknown(arg));
            }
            Ok(match name.as_str() {
                "stop" => Command::Service(Action::Stop),
                "status" => Command::Service(Action::Status),
                _ => Command::Window,
            })
        }
        "server" | "dev" | "chat" => {
            let mut port = if name == "dev" { 0 } else { 7313 };
            let mut faux = None;
            let mut home = None;
            let mut isolation = Isolation::Sandboxed;
            let mut to = hamlet_protocol::MAYOR.to_owned();
            let mut flags = rest.iter();
            while let Some(flag) = flags.next() {
                let flag = flag.as_str();
                if flag == "--insecure" && name != "chat" {
                    isolation = insecure()?;
                    continue;
                }
                let needs = match flag {
                    "--port" if name != "chat" => "a port number",
                    "--faux" if name != "chat" => {
                        "the path of a script"
                    }
                    "--home" if name == "dev" => {
                        "the path of a scratch home"
                    }
                    "--to" if name == "chat" => "an agent's name",
                    _ => return Err(unknown(flag)),
                };
                let value = flags.next().ok_or_else(|| {
                    anyhow::anyhow!("{flag} needs {needs}")
                })?;
                match flag {
                    "--port" => {
                        if value.is_empty()
                            || !value
                                .bytes()
                                .all(|b| b.is_ascii_digit())
                        {
                            bail!(
                                "--port needs a port number, not \
                                 `{value}`"
                            );
                        }
                        port =
                            value.parse::<u16>().map_err(|_| {
                                anyhow::anyhow!(
                                    "--port needs a port number, \
                                     not `{value}`"
                                )
                            })?;
                    }
                    "--faux" => faux = Some(value.clone()),
                    "--home" => home = Some(value.clone()),
                    "--to" => to = value.clone(),
                    _ => unreachable!(),
                }
            }
            Ok(match name.as_str() {
                "server" => Command::Server {
                    port,
                    faux,
                    isolation,
                },
                "dev" => Command::Dev {
                    port,
                    faux,
                    home,
                    isolation,
                },
                _ => Command::Chat(to),
            })
        }
        "edit" => {
            let doc = rest
                .first()
                .filter(|s| {
                    matches!(
                        s.as_str(),
                        "charter"
                            | "character"
                            | "notes"
                            | "limits"
                            | "settings"
                    )
                })
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "edit needs one of charter, character, \
                         notes, limits or settings"
                    )
                })?;
            let hamlet = if rest.len() == 1 {
                hamlet_protocol::HOME
            } else {
                if doc != "limits" || rest[1] != "--hamlet" {
                    return Err(unknown(&rest[1]));
                }
                if rest.len() > 3 {
                    return Err(unknown(&rest[3]));
                }
                rest.get(2)
                    .filter(|s| !s.is_empty() && !s.starts_with("--"))
                    .ok_or_else(|| {
                        anyhow::anyhow!("--hamlet needs a hamlet id")
                    })?
            };
            Ok(Command::Terminal(terminal::Command::Edit {
                doc: doc.clone(),
                hamlet: hamlet.into(),
            }))
        }
        "login" => {
            let provider = rest
                .first()
                .filter(|s| !s.is_empty() && !s.starts_with("--"))
                .ok_or_else(|| {
                    anyhow::anyhow!("login needs a provider")
                })?;
            if rest.len() > 2 {
                return Err(unknown(&rest[2]));
            }
            if let Some(flag) = rest.get(1)
                && flag != "--key"
            {
                return Err(unknown(flag));
            }
            Ok(Command::Terminal(terminal::Command::Login {
                provider: provider.clone(),
                key: rest.len() == 2,
            }))
        }
        "models" => {
            let action =
                rest.first().map(String::as_str).unwrap_or("list");
            if !(rest.is_empty()
                || action == "try" && rest.len() <= 2
                || action == "refresh" && rest.len() == 1)
            {
                return Err(unknown(
                    rest.last().map(String::as_str).unwrap_or(action),
                ));
            }
            Ok(Command::Terminal(terminal::Command::Models {
                action: action.into(),
                model: rest.get(1).cloned(),
            }))
        }
        _ => Err(unknown(name)),
    }
}

/// `--insecure`, which only Windows takes, as it has no sandbox yet
/// (C18).
fn insecure() -> Result<Isolation> {
    if cfg!(windows) {
        Ok(Isolation::Insecure)
    } else {
        bail!(
            "Insecure mode is only for Windows. This platform has a \
             sandbox."
        )
    }
}

fn main() {
    let result = (|| -> Result<i32> {
        let command =
            parse(&std::env::args().skip(1).collect::<Vec<_>>())?;
        if matches!(command, Command::Help) {
            println!("{USAGE}");
            return Ok(0);
        }
        if let Command::Server {
            port,
            faux,
            isolation,
        } = command
        {
            return process::serve(port, faux.as_deref(), isolation);
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let result = runtime.block_on(async {
            match command {
                Command::Service(action) => {
                    process::service(action).await
                }
                Command::Dev {
                    port,
                    faux,
                    home,
                    isolation,
                } => {
                    process::develop(
                        port,
                        faux.as_deref(),
                        home.as_deref(),
                        isolation,
                    )
                    .await
                }
                Command::Window => process::window().await,
                Command::Chat(to) => {
                    chat::run(&process::address()?, &to).await?;
                    Ok(0)
                }
                Command::Terminal(command) => {
                    terminal::run(command, &process::address()?)
                        .await?;
                    Ok(0)
                }
                Command::Help | Command::Server { .. } => {
                    unreachable!()
                }
            }
        });
        // A blocked terminal read must not keep an already closed
        // client alive.
        runtime.shutdown_background();
        result
    })();
    match result {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("Error: {error:#}");
            std::process::exit(1);
        }
    }
}
