//! Document editing, sign-in and model queries through the server
//! protocol.

#[cfg(unix)]
use std::io::IsTerminal;
use std::io::Write;
use std::path::PathBuf;
use std::{env, fs};

use anyhow::{Context, Result, bail};
use hamlet_protocol::client::*;
#[cfg(unix)]
use rustix::termios::{
    LocalModes, OptionalActions, Termios, tcgetattr, tcsetattr,
};
use tokio::io::{AsyncBufReadExt, BufReader};
use uuid::Uuid;

use crate::client::{self, Client, ServerAddress};

/// A terminal operation performed over an authenticated chief
/// connection.
pub enum Command {
    Edit {
        doc: String,
        /// The hamlet whose limits to edit; the realm's when absent.
        hamlet: Option<String>,
        /// The scholar or agent whose character or memory to edit.
        whom: Option<String>,
        /// The memory file, under `memory/`.
        path: Option<String>,
    },
    Login {
        provider: String,
        key: bool,
    },
    Models {
        action: String,
        model: Option<String>,
    },
}

/// Runs a terminal operation, leaving agents running when its
/// connection closes.
pub async fn run(
    command: Command,
    address: &ServerAddress,
) -> Result<()> {
    let (mut client, state) = client::connect(address).await?;
    let result = match command {
        Command::Login { provider, key } => {
            login(&mut client, provider, key).await
        }
        Command::Models { action, model } => {
            models(&mut client, &action, model).await
        }
        Command::Edit {
            doc,
            hamlet,
            whom,
            path,
        } => {
            edit(
                &mut client,
                &state,
                &doc,
                hamlet.as_deref(),
                whom.as_deref(),
                path.as_deref(),
            )
            .await
        }
    };
    let _ = client.close().await;
    result
}

fn checked(message: ServerMessage) -> Result<ServerMessage> {
    match message {
        ServerMessage::Fault(fault) => bail!("{}", fault.text),
        ServerMessage::Reply(Reply {
            kind: ReplyKind::Error,
            text,
            ..
        }) => bail!("{text}"),
        message => Ok(message),
    }
}

async fn models(
    client: &mut Client,
    action: &str,
    model: Option<String>,
) -> Result<()> {
    let id = RequestId::new();
    let message = if action == "try" {
        ClientMessage::Check(Check::Model {
            id: id.clone(),
            model,
        })
    } else {
        ClientMessage::Read(Read::Models {
            id: id.clone(),
            refresh: Some(action == "refresh"),
        })
    };
    match checked(client.request(&id, message).await?)? {
        ServerMessage::Reply(reply) => println!("{}", reply.text),
        ServerMessage::Read(ReadResult::Models {
            models, ..
        }) => {
            for model in models {
                let price = model
                    .price
                    .map(|price| {
                        format!(
                            "${}/M in, ${}/M out",
                            price.input, price.output
                        )
                    })
                    .unwrap_or("unpriced".into());
                let context = model
                    .context
                    .map(|c| c.to_string())
                    .unwrap_or("unknown".into());
                let note = if model.note.is_empty() {
                    String::new()
                } else {
                    format!("\t{}", model.note)
                };
                println!(
                    "{}\t{price}\t{context} context{note}",
                    model.name
                );
            }
        }
        _ => bail!("The server did not send the models."),
    }
    Ok(())
}

/// The scholar or agent `whom` addresses, for its character or
/// memory: `scholar#<id>` or an agent's address.
fn keeper(state: &State, whom: &str) -> Result<Keeper> {
    if let Some(id) = whom.strip_prefix("scholar#") {
        let scholar = id
            .parse::<u64>()
            .ok()
            .and_then(|id| {
                state.scholars.iter().find(|s| {
                    s.id == ScholarId(id) && s.left.is_none()
                })
            })
            .with_context(|| format!("no scholar at `{whom}`"))?;
        return Ok(Keeper::Scholar(scholar.id));
    }
    Ok(Keeper::Agent(crate::chat::resolve(state, whom)?))
}

/// The scholar whose character `whom` names: a scholar, or the
/// scholar a hire works as.
fn scholar(state: &State, whom: &str) -> Result<ScholarId> {
    match keeper(state, whom)? {
        Keeper::Scholar(id) => Ok(id),
        Keeper::Agent(id) => state
            .agents
            .iter()
            .find(|a| a.id == id)
            .and_then(|a| a.scholar)
            .with_context(|| format!("`{whom}` works as no scholar")),
    }
}

async fn edit(
    client: &mut Client,
    state: &State,
    doc: &str,
    hamlet: Option<&str>,
    whom: Option<&str>,
    path: Option<&str>,
) -> Result<()> {
    let whom = || {
        whom.with_context(|| {
            format!("edit {doc} needs a scholar or agent address")
        })
    };
    let path = path.unwrap_or("notes.md");
    let (of, scholar) = match doc {
        "character" => {
            let scholar = scholar(state, whom()?)?;
            (Keeper::Scholar(scholar), Some(scholar))
        }
        "memory" => (keeper(state, whom()?)?, None),
        _ => (Keeper::Agent(state.hand), None),
    };
    let text = match doc {
        "limits" => match hamlet {
            Some(hamlet) => {
                let hamlet = state
                    .hamlets
                    .iter()
                    .find(|h| h.id == hamlet)
                    .with_context(|| {
                        format!("no hamlet named `{hamlet}`")
                    })?;
                format!(
                    "{}\n",
                    serde_json::to_string_pretty(&hamlet.limits)?
                )
            }
            None => format!(
                "{}\n",
                serde_json::to_string_pretty(&state.limits)?
            ),
        },
        "settings" => {
            let settings = &state.settings;
            let endpoints = settings
                .endpoints
                .iter()
                .map(|e| Endpoint {
                    name: e.name.clone(),
                    base_url: e.base_url.clone(),
                })
                .collect();
            format!(
                "{}\n",
                serde_json::to_string_pretty(&EditSettingsSet {
                    allow_net: Some(settings.allow_net),
                    character: Some(settings.character.clone()),
                    effort: Some(settings.effort),
                    newcomers: Some(settings.newcomers),
                    reflect_every: Some(settings.reflect_every),
                    model: Some(settings.model.clone()),
                    endpoints: Some(endpoints),
                    workspace: None,
                })?
            )
        }
        _ => {
            let id = RequestId::new();
            let read = if doc == "charter" {
                Read::Charter { id: id.clone() }
            } else {
                Read::Papers {
                    id: id.clone(),
                    of,
                    full: Some(true),
                }
            };
            match checked(
                client
                    .request(&id, ClientMessage::Read(read))
                    .await?,
            )? {
                ServerMessage::Read(ReadResult::Charter {
                    charter,
                    ..
                }) if doc == "charter" => charter.text,
                ServerMessage::Read(ReadResult::Papers {
                    papers,
                    ..
                }) => {
                    if doc == "memory" {
                        papers
                            .memory
                            .into_iter()
                            .find(|file| file.path == path)
                            .map(|file| file.text)
                            .unwrap_or_default()
                    } else {
                        papers.character
                    }
                }
                _ => bail!("The server did not send the document."),
            }
        }
    };
    let text = in_editor(doc, &text).await?;
    let id = RequestId::new();
    let message = match (doc, scholar) {
        ("charter", _) => Edit::Charter {
            id: id.clone(),
            text: Some(text),
        },
        ("character", Some(scholar)) => Edit::Character {
            id: id.clone(),
            scholar,
            text,
        },
        ("memory", _) => Edit::Memory {
            id: id.clone(),
            of,
            path: path.into(),
            text,
        },
        ("limits", _) => Edit::Limits(EditLimits::Whole {
            id: id.clone(),
            hamlet: hamlet.map(str::to_owned),
            whole: serde_json::from_str(&text)
                .context("Invalid limits JSON; nothing was saved")?,
            reason: None,
        }),
        ("settings", _) => Edit::Settings {
            id: id.clone(),
            set: serde_json::from_str::<EditSettingsSet>(&text)
                .context(
                    "Invalid settings JSON; nothing was saved",
                )?,
        },
        _ => bail!("Unknown document"),
    };
    match checked(
        client.request(&id, ClientMessage::Edit(message)).await?,
    )? {
        ServerMessage::Reply(reply) => {
            eprintln!("  ({})", reply.text);
        }
        _ => bail!("The server did not answer the edit."),
    }
    Ok(())
}

struct EditDirectory(PathBuf);
impl Drop for EditDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

async fn in_editor(doc: &str, text: &str) -> Result<String> {
    #[cfg(unix)]
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    let editor = env::var("EDITOR")
        .ok()
        .filter(|s| !s.is_empty())
        .context("Set EDITOR to the editor to open.")?;
    let path = env::temp_dir()
        .join(format!("hamlet-edit-{}", Uuid::new_v4()));
    #[cfg(unix)]
    fs::DirBuilder::new().mode(0o700).create(&path)?;
    #[cfg(not(unix))]
    fs::create_dir(&path)?;
    let dir = EditDirectory(path);
    let path = dir.0.join(format!(
        "{doc}.{}",
        if doc == "limits" || doc == "settings" {
            "json"
        } else {
            "md"
        }
    ));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    options.open(&path)?.write_all(text.as_bytes())?;
    let status = tokio::process::Command::new("sh")
        .args(["-c", &format!("exec {editor} \"$1\""), "hamlet-edit"])
        .arg(&path)
        .status()
        .await?;
    anyhow::ensure!(
        status.success(),
        "The editor ended with {}. Nothing was saved.",
        status
            .code()
            .map(|n| n.to_string())
            .unwrap_or_else(|| "a signal".into())
    );
    Ok(fs::read_to_string(path)?)
}

#[cfg(unix)]
struct PrivateInput(Option<Termios>);

#[cfg(unix)]
impl PrivateInput {
    fn new() -> Result<Self> {
        let stdin = std::io::stdin();
        if !stdin.is_terminal() {
            return Ok(Self(None));
        }
        let original = tcgetattr(&stdin)?;
        let mut quiet = original.clone();
        // Canonical editing stays enabled.
        quiet
            .local_modes
            .remove(LocalModes::ECHO | LocalModes::ECHONL);
        tcsetattr(&stdin, OptionalActions::Now, &quiet)?;
        Ok(Self(Some(original)))
    }
}

#[cfg(unix)]
impl Drop for PrivateInput {
    fn drop(&mut self) {
        if let Some(original) = &self.0 {
            // A terminal that cannot be restored is left as it is.
            let _ = tcsetattr(
                std::io::stdin(),
                OptionalActions::Now,
                original,
            );
        }
    }
}

#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "the console's echo is set through kernel32"
)]
mod console {
    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle;

    use anyhow::Result;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetConsoleMode(handle: *mut c_void, mode: *mut u32)
        -> i32;
        fn SetConsoleMode(handle: *mut c_void, mode: u32) -> i32;
    }

    pub(super) struct PrivateInput(Option<u32>);

    impl PrivateInput {
        pub(super) fn new() -> Result<Self> {
            use std::io::IsTerminal;
            if !std::io::stdin().is_terminal() {
                return Ok(Self(None));
            }
            let handle = std::io::stdin().as_raw_handle();
            let mut mode = 0;
            // SAFETY: stdin owns the console handle and mode is
            // writable storage.
            if unsafe { GetConsoleMode(handle, &mut mode) } == 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            // SAFETY: preserve the console mode except
            // ENABLE_ECHO_INPUT; the handle stays open.
            if unsafe { SetConsoleMode(handle, mode & !0x0004) } == 0
            {
                return Err(std::io::Error::last_os_error().into());
            }
            Ok(Self(Some(mode)))
        }
    }

    impl Drop for PrivateInput {
        fn drop(&mut self) {
            if let Some(mode) = self.0 {
                // SAFETY: restore the mode read from the process's
                // still-open stdin handle.
                unsafe {
                    SetConsoleMode(
                        std::io::stdin().as_raw_handle(),
                        mode,
                    );
                }
            }
        }
    }
}

async fn login(
    client: &mut Client,
    provider: String,
    key: bool,
) -> Result<()> {
    #[cfg(unix)]
    let _quiet = PrivateInput::new()?;
    #[cfg(windows)]
    let _quiet = console::PrivateInput::new()?;
    let mut input = BufReader::new(tokio::io::stdin()).lines();
    client
        .send(ClientMessage::Login(Login::Start {
            id: RequestId::new(),
            start: LoginStart {
                provider,
                method: if key {
                    LoginMethod::Key
                } else {
                    LoginMethod::Subscription
                },
            },
        }))
        .await?;
    loop {
        let message = tokio::select! {
            message = client.next() => checked(message?)?,
            _ = tokio::signal::ctrl_c() => bail!("Sign-in cancelled."),
        };
        let ServerMessage::Login(message) = message else {
            continue;
        };
        match message.step {
            LoginStep::Url { url, instructions } => {
                eprintln!("{url}");
                if let Some(instructions) = instructions {
                    eprintln!("{instructions}");
                }
            }
            LoginStep::Code { code } => eprintln!("Code: {code}"),
            LoginStep::Progress { progress } => {
                eprintln!("{progress}");
            }
            LoginStep::Failed { failed } => bail!("{failed}"),
            LoginStep::Done { done } => {
                eprintln!("Signed in to {done}.");
                return Ok(());
            }
            LoginStep::Prompt { prompt, .. } => {
                eprintln!("{prompt}");
                let value = tokio::select! {
                    value = input.next_line() => value?.context("Sign-in needs an answer; input ended.")?,
                    _ = tokio::signal::ctrl_c() => bail!("Sign-in cancelled."),
                };
                client
                    .send(ClientMessage::Login(Login::Reply {
                        id: RequestId::new(),
                        reply: LoginReply {
                            login: message.login,
                            value,
                        },
                    }))
                    .await?;
            }
        }
    }
}
