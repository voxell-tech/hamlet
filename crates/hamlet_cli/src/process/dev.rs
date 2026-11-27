//! `hamlet dev`: the checkout's server and window, built and run on
//! a scratch home until the window closes.

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use std::{env, fs};

use anyhow::{Context, Result, bail};
#[cfg(unix)]
use hamlet_os::process::own_session;
use hamlet_protocol::client::discovery;
#[cfg(unix)]
use rustix::process::Signal;
use tokio::process::Child;
use tokio::time::{sleep, timeout};

#[cfg(unix)]
use super::signal;
#[cfg(windows)]
use super::{CREATE_NEW_PROCESS_GROUP, kernel32, own_session};
use super::{
    Isolation, Stops, exit_code, is_answering, mkdir, nonempty, repo,
    running, server_command, window_command,
};
use crate::client::ServerAddress;

fn canonical(path: &Path) -> Result<PathBuf> {
    match fs::canonicalize(path) {
        Ok(path) => Ok(path),
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound =>
        {
            let parent =
                path.parent().context("No parent for home")?;
            Ok(canonical(parent)?
                .join(path.file_name().context("No name for home")?))
        }
        Err(error) => Err(error.into()),
    }
}

fn dev_home(chosen: Option<&str>) -> Result<PathBuf> {
    // On Unix the temp folder is shared by every account.
    #[cfg(unix)]
    let scratch =
        format!("hamlet-dev-{}", rustix::process::getuid().as_raw());
    #[cfg(windows)]
    let scratch = "hamlet-dev";
    let path = chosen
        .map(PathBuf::from)
        .or_else(|| nonempty("HAMLET_HOME").map(PathBuf::from))
        .unwrap_or_else(|| env::temp_dir().join(scratch));
    let path = std::path::absolute(&path)?;
    let ordinary = discovery::home(|name| {
        if name == "HAMLET_HOME" {
            None
        } else {
            nonempty(name)
        }
    })?;
    anyhow::ensure!(
        chosen != Some("~/.hamlet")
            && canonical(&path)? != canonical(&ordinary)?,
        "hamlet dev refuses ~/.hamlet; choose a scratch realm with \
         --home or HAMLET_HOME."
    );
    if !path.exists() {
        mkdir(&path)?;
    }
    let stat = fs::symlink_metadata(&path)?;
    let unsafe_reason = if stat.is_symlink() {
        Some("a symbolic link")
    } else if !stat.is_dir() {
        Some("not a directory")
    } else {
        shared(&stat)
    };
    if let Some(reason) = unsafe_reason {
        bail!("unsafe scratch realm {}: {reason}.", path.display());
    }
    Ok(path)
}

/// Why a scratch home's folder is open to other accounts, if it is.
#[cfg(unix)]
fn shared(stat: &fs::Metadata) -> Option<&'static str> {
    if stat.uid() != rustix::process::getuid().as_raw() {
        Some("not owned by the current user")
    } else if stat.mode() & 0o022 != 0 {
        Some("writable by group or others")
    } else {
        None
    }
}

/// Why a scratch home's folder is open to other accounts: never, as
/// Windows gives a folder made under the profile its owner's access.
#[cfg(windows)]
fn shared(_: &fs::Metadata) -> Option<&'static str> {
    None
}

/// The checkout's `name` executable as Cargo builds it, with the
/// cargo `features` given, else the one `variable` names.
async fn dev_binary(
    name: &str,
    variable: &str,
    features: &[&str],
) -> Result<PathBuf> {
    if let Some(path) = nonempty(variable) {
        return Ok(path.into());
    }
    let root = repo();
    anyhow::ensure!(
        root.join("Cargo.toml").is_file(),
        "hamlet dev needs a source checkout or {variable}."
    );
    eprintln!("hamlet: building {name}");
    let mut command = tokio::process::Command::new("cargo");
    command
        .current_dir(&root)
        .args([
            "build",
            "--bin",
            name,
            "--message-format=json-render-diagnostics",
        ])
        .args(features.iter().flat_map(|f| ["--features", f]))
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    // `cargo run` set these for this process. Build scripts such as
    // ring's track them, so inheriting them rebuilds ring, rustls and
    // reqwest on every `hamlet dev` under `cargo run`.
    for (variable, _) in env::vars_os() {
        let leaked = variable.to_str().is_some_and(|variable| {
            variable.starts_with("CARGO_PKG_")
                || variable.starts_with("CARGO_BIN_")
                || matches!(
                    variable,
                    "CARGO_MANIFEST_DIR"
                        | "CARGO_MANIFEST_PATH"
                        | "CARGO_CRATE_NAME"
                        | "CARGO_PRIMARY_PACKAGE"
                )
        });
        if leaked {
            command.env_remove(variable);
        }
    }
    // Not `output`, which pipes stderr too and hides the progress.
    let output = command
        .spawn()
        .with_context(|| {
            format!("Cannot build {name}; install Rust and Cargo")
        })?
        .wait_with_output()
        .await?;
    anyhow::ensure!(
        output.status.success(),
        "The {name} build failed; no server was started."
    );
    for line in output.stdout.split(|b| *b == b'\n') {
        if let Ok(value) =
            serde_json::from_slice::<serde_json::Value>(line)
            && value["reason"] == "compiler-artifact"
            && value["target"]["name"] == name
            && let Some(path) = value["executable"].as_str()
        {
            return Ok(path.into());
        }
    }
    bail!("Cargo did not report the built {name} executable.")
}

async fn wait_child(child: &mut Child) -> Result<()> {
    timeout(Duration::from_secs(30), child.wait())
        .await
        .context(
            "Child is still stopping after 30 seconds; it was not \
             killed",
        )??;
    Ok(())
}

/// The address of the dev server `server` once it answers there.
async fn ready(
    server: &mut Child,
    home: &Path,
) -> Result<ServerAddress> {
    loop {
        if let Some(status) = server.try_wait()? {
            bail!(
                "The server exited before becoming ready ({status})."
            );
        }
        if let Ok(Some(record)) = running(home)
            && Some(record.pid) == server.id()
        {
            let address = record.address(home)?;
            if is_answering(&address).await {
                return Ok(address);
            }
        }
        sleep(Duration::from_millis(50)).await;
    }
}

/// Forgets the dev window once it closed, saying the server still
/// serves.
fn window_closed(
    window: &mut Option<Child>,
    home: &Path,
    address: &ServerAddress,
) -> Result<()> {
    if let Some(child) = window
        && child.try_wait()?.is_some()
    {
        *window = None;
        eprintln!(
            "hamlet: window closed; still serving {} at {}; Ctrl-C \
             stops",
            home.display(),
            address.url
        );
    }
    Ok(())
}

/// Serves a scratch home from the checkout's server and opens the
/// checkout's window on it. With `hot`, the window reloads saved
/// shaders and tuning files.
pub async fn develop(
    port: u16,
    faux: Option<&str>,
    chosen: Option<&str>,
    isolation: Isolation,
    hot: bool,
) -> Result<i32> {
    let home = dev_home(chosen)?;
    let server =
        dev_binary("hamlet_server", "HAMLET_SERVER_BIN", &[]).await?;
    let features: &[&str] =
        if hot { &["hamlet_app/hot-reload"] } else { &[] };
    let app =
        dev_binary("hamlet_app", "HAMLET_APP", features).await?;
    let mut stops = Stops::new()?;
    let mut command =
        server_command(&server, &home, port, faux, isolation);
    // The server is kept from the terminal's Ctrl-C, and this
    // process asks it to stop.
    #[cfg(unix)]
    own_session(&mut command);
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(
        &mut command,
        CREATE_NEW_PROCESS_GROUP,
    );
    command.stdin(Stdio::null());
    let mut server =
        tokio::process::Command::from(command).spawn()?;
    let started = server
        .id()
        .map_or_else(|| "?".to_string(), |pid| pid.to_string());
    eprintln!(
        "hamlet: started the server (pid {started}) on {}; waiting \
         up to 15 s for it to answer",
        home.display()
    );
    let mut window = None;
    let result = async {
        let ready = ready(&mut server, &home);
        let ready = timeout(Duration::from_secs(15), ready);
        let address = tokio::select! {
            ready = ready => ready.with_context(|| {
                format!(
                    "The server (pid {started}) did not become ready \
                     within 15 seconds; its home is {}",
                    home.display()
                )
            })??,
            () = stops.next() => return Ok(0),
        };
        eprintln!(
            "hamlet: the server answers at {}; opening the window",
            address.url
        );
        let mut command = window_command(&app, &address);
        command.env("HAMLET_VIEW", home.join("window"));
        // Bevy watches the files under CARGO_MANIFEST_DIR, which must
        // be the checkout for it to see the saved shaders and tuning.
        if hot {
            command.env("CARGO_MANIFEST_DIR", repo());
        }
        own_session(&mut command);
        let child = tokio::process::Command::from(command)
            .spawn()
            .with_context(|| {
                format!("Cannot open {}", app.display())
            })?;
        window = Some(child);
        loop {
            tokio::select! {
                () = stops.next() => return Ok(0),
                status = server.wait() => {
                    return Ok(exit_code(status?));
                }
                _ = sleep(Duration::from_millis(50)) => {
                    window_closed(&mut window, &home, &address)?;
                }
            }
        }
    }
    .await;
    #[cfg(unix)]
    for pid in [window.as_ref().and_then(Child::id), server.id()]
        .into_iter()
        .flatten()
    {
        signal(pid, Signal::TERM)?;
    }
    #[cfg(windows)]
    {
        // The window has no console to take an event on.
        if let Some(child) = &mut window {
            end(child)?;
        }
        if let Some(pid) = server.id() {
            kernel32::interrupt(pid)?;
        }
    }
    let closing = async {
        let window_closed = async {
            if let Some(child) = &mut window {
                wait_child(child).await?;
            }
            Ok::<_, anyhow::Error>(())
        };
        tokio::try_join!(window_closed, wait_child(&mut server))?;
        Ok::<_, anyhow::Error>(())
    };
    tokio::select! {
        closed = closing => closed?,
        () = stops.next() => {
            if let Some(child) = &mut window {
                end(child)?;
                child.wait().await?;
            }
            end(&mut server)?;
            server.wait().await?;
            return Ok(130);
        }
    }
    result
}

/// Kills `child`, unless it has been waited for.
fn end(child: &mut Child) -> std::io::Result<()> {
    if child.id().is_some() {
        child.start_kill()
    } else {
        Ok(())
    }
}
