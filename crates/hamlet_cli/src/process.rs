//! Local server discovery and the lifetimes of the server and window.

#![cfg_attr(
    unix,
    expect(
        unsafe_code,
        reason = "a session's leader is made between fork and exec"
    )
)]

use std::env;
#[cfg(unix)]
use std::fs;
#[cfg(unix)]
use std::io::{Read, Seek, SeekFrom};
#[cfg(unix)]
use std::os::unix::{
    fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    process::{CommandExt, ExitStatusExt},
};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::Duration;

use anyhow::{Context, Result, bail};
#[cfg(unix)]
use hamlet_protocol::client::Confinement;
#[cfg(unix)]
use hamlet_protocol::client::discovery::Running;
use hamlet_protocol::client::discovery::{self, Chosen};
#[cfg(unix)]
use rustix::io::Errno;
#[cfg(unix)]
use rustix::process::{Pid, Signal, kill_process, test_kill_process};
use tokio::time::timeout;
#[cfg(unix)]
use tokio::{
    process::Child,
    time::{Instant, sleep},
};

use crate::client::{self, ServerAddress};

/// What `hamlet`, `hamlet stop` and `hamlet status` do with the
/// local server.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Starts the server, unless one runs, and opens a window on it.
    Start(Isolation),
    Stop,
    Status,
}

/// Whether a server the CLI starts runs agents' commands in a
/// sandbox.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Isolation {
    Sandboxed,
    /// `--insecure`, which only Windows takes.
    Insecure,
}

fn nonempty(name: &str) -> Option<String> {
    env::var(name).ok().filter(|s| !s.is_empty())
}

/// The local home the environment selects.
#[cfg(unix)]
fn home() -> Result<PathBuf> {
    Ok(discovery::home(nonempty)?)
}

/// The server the environment selects.
pub fn address() -> Result<ServerAddress> {
    Ok(ServerAddress::find(nonempty)?)
}

#[cfg(unix)]
fn running(home: &Path) -> Result<Option<Running>> {
    Ok(Running::read(home)?)
}

async fn is_answering(address: &ServerAddress) -> bool {
    timeout(Duration::from_secs(2), async {
        let (mut connection, _) = client::connect(address).await?;
        connection.close().await
    })
    .await
    .is_ok_and(|result| result.is_ok())
}

/// The one process `pid` names, never a process group.
#[cfg(unix)]
fn process(pid: u32) -> Option<Pid> {
    i32::try_from(pid).ok().and_then(Pid::from_raw)
}

#[cfg(unix)]
fn is_alive(pid: u32) -> bool {
    // EPERM says the process exists but is not ours to signal.
    process(pid)
        .is_some_and(|pid| test_kill_process(pid) != Err(Errno::SRCH))
}

#[cfg(unix)]
fn signal(pid: u32, signal: Signal) -> Result<()> {
    let pid = process(pid).context("Invalid process ID")?;
    match kill_process(pid, signal) {
        Err(error) if error != Errno::SRCH => {
            Err(std::io::Error::from(error).into())
        }
        _ => Ok(()),
    }
}

#[cfg(unix)]
async fn inspect(
    home: &Path,
) -> Result<Option<(Running, ServerAddress)>> {
    let Some(record) = running(home)? else {
        return Ok(None);
    };
    if !is_alive(record.pid) {
        return Ok(None);
    }
    if let Ok(address) = record.address(home)
        && is_answering(&address).await
    {
        return Ok(Some((record, address)));
    }
    if !is_alive(record.pid) {
        return Ok(None);
    }
    bail!(
        "Process {} for {} is alive but does not answer with the \
         home's token; it is not ours to touch. Inspect it on its \
         machine; no signal was sent.",
        record.pid,
        home.display()
    )
}

#[cfg(unix)]
fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[cfg(unix)]
fn server_command(
    server: &Path,
    home: &Path,
    port: u16,
    faux: Option<&str>,
    isolation: Isolation,
) -> Command {
    let mut command = Command::new(server);
    command
        .arg("--port")
        .arg(port.to_string())
        .env("HAMLET_HOME", home);
    if let Some(faux) = faux {
        command.arg("--faux").arg(faux);
    }
    if isolation == Isolation::Insecure {
        command.arg("--insecure");
    }
    command
}

#[cfg(unix)]
pub fn serve(
    port: u16,
    faux: Option<&str>,
    isolation: Isolation,
) -> Result<i32> {
    let server = find_binary("hamlet_server", "HAMLET_SERVER_BIN")?;
    let mut command =
        server_command(&server, &home()?, port, faux, isolation);
    Err(command.exec()).context(
        "Cannot start hamlet_server; build it or set \
         HAMLET_SERVER_BIN",
    )
}

#[cfg(unix)]
fn session(command: &mut Command) {
    // SAFETY: the child calls only setsid, which is
    // async-signal-safe, before exec, allocates nothing and touches
    // no shared state.
    unsafe {
        command.pre_exec(|| {
            rustix::process::setsid()?;
            Ok(())
        });
    }
}

#[cfg(windows)]
fn session(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    command.creation_flags(0x0000_0008 | 0x0000_0200);
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        fs::metadata(path)
            .is_ok_and(|m| m.is_file() && m.mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

fn find_binary(name: &str, variable: &str) -> Result<PathBuf> {
    if let Some(path) = nonempty(variable) {
        return Ok(path.into());
    }
    // Prefer the binary built beside this CLI, before an unrelated
    // installation on PATH.
    if let Some(dir) = env::current_exe()?.parent() {
        let path =
            dir.join(format!("{name}{}", env::consts::EXE_SUFFIX));
        if is_executable(&path) {
            return Ok(path);
        }
    }
    for dir in
        env::split_paths(&env::var_os("PATH").unwrap_or_default())
    {
        let path =
            dir.join(format!("{name}{}", env::consts::EXE_SUFFIX));
        if is_executable(&path) {
            return Ok(path);
        }
    }
    bail!(
        "No {name} found. Build it with cargo build --bin {name}, \
         or set {variable}."
    )
}

fn window_command(app: &Path, address: &ServerAddress) -> Command {
    let mut command = Command::new(app);
    command
        .env("HAMLET_SERVER", &address.url)
        .env("HAMLET_TOKEN", &address.token);
    command
}

fn open_detached(address: &ServerAddress) -> Result<()> {
    let opened = (|| -> Result<()> {
        let mut command = window_command(
            &find_binary("hamlet_app", "HAMLET_APP")?,
            address,
        );
        session(&mut command);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        Ok(())
    })();
    opened.context(
        "Cannot open a window. Build hamlet_app or set HAMLET_APP \
         to its executable, then open again. No stop was requested; \
         hamlet status reports the server.",
    )
}

fn exit_code(status: ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        return code;
    }
    #[cfg(unix)]
    {
        128 + status.signal().unwrap_or(1)
    }
    #[cfg(not(unix))]
    {
        1
    }
}

pub async fn window() -> Result<i32> {
    let app = find_binary("hamlet_app", "HAMLET_APP")?;
    let command = window_command(&app, &address()?);
    Ok(exit_code(
        tokio::process::Command::from(command)
            .status()
            .await
            .with_context(|| {
                format!("Cannot open {}", app.display())
            })?,
    ))
}

#[cfg(unix)]
fn log_tail(home: &Path) -> String {
    let read = (|| -> Result<String> {
        let mut file = fs::File::open(home.join("server.log"))?;
        let start = file.metadata()?.len().saturating_sub(8192);
        file.seek(SeekFrom::Start(start))?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        let mut text = String::from_utf8_lossy(&bytes).into_owned();
        let mut secrets = Vec::new();
        for name in ["HAMLET_API_KEY", "HAMLET_TOKEN"] {
            if let Some(secret) = nonempty(name) {
                secrets.push(secret);
            }
        }
        match fs::read(home.join("credentials.json")) {
            Ok(bytes) => {
                let stored = serde_json::from_slice::<
                    serde_json::Value,
                >(&bytes)?;
                let token = stored["connection"]["token"]
                    .as_str()
                    .context("Invalid connection token")?;
                if !token.is_empty() {
                    secrets.push(token.to_owned());
                }
                let models = stored["models"]
                    .as_object()
                    .context("Invalid model credentials")?;
                for credential in models.values() {
                    let fields = match credential["type"].as_str() {
                        Some("api_key") => &["key"][..],
                        Some("oauth") => &["access", "refresh"][..],
                        _ => bail!("Unknown credential type"),
                    };
                    for field in fields {
                        if let Some(value) = credential.get(field) {
                            let secret = value.as_str().context(
                                "Invalid credential value",
                            )?;
                            if !secret.is_empty() {
                                secrets.push(secret.to_owned());
                            }
                        }
                    }
                }
            }
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        secrets.sort_by_key(|s| std::cmp::Reverse(s.len()));
        for secret in secrets {
            text = text.replace(&secret, "[REDACTED]");
        }
        Ok(text.trim_end().into())
    })();
    read.unwrap_or_else(|_| {
        "The end of server.log could not be read safely.".into()
    })
}

#[cfg(unix)]
fn mkdir(home: &Path) -> Result<()> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(home)?;
    Ok(())
}

/// Starts a server for `home`, or finds the one another `hamlet`
/// started meanwhile.
#[cfg(unix)]
async fn start(
    home: &Path,
    isolation: Isolation,
) -> Result<(Running, ServerAddress)> {
    mkdir(home)?;
    let log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(home.join("server.log"))?;
    let log_start = log.metadata()?.len();
    let server = find_binary("hamlet_server", "HAMLET_SERVER_BIN")?;
    let mut command =
        server_command(&server, home, 0, None, isolation);
    session(&mut command);
    command
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    let mut child = command.spawn().context(
        "Cannot start the server process; check HAMLET_SERVER_BIN",
    )?;
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if let Ok(Some(found)) = inspect(home).await {
            return Ok(found);
        }
        if child.try_wait()?.is_some() {
            let recent =
                fs::read(home.join("server.log")).unwrap_or_default();
            let recent = String::from_utf8_lossy(
                recent.get(log_start as usize..).unwrap_or_default(),
            );
            if !recent.contains("A server already runs for ")
                && !recent.contains("EADDRINUSE")
            {
                bail!(
                    "Cannot start the server: it exited before \
                     becoming ready. Inspect {} and hamlet status \
                     before trying again.\n{}",
                    home.join("server.log").display(),
                    log_tail(home)
                );
            }
        }
        sleep(Duration::from_millis(100)).await;
    }
    bail!(
        "Cannot start the server: it did not become ready within 15 \
         seconds. Inspect {} and hamlet status before trying \
         again.\n{}",
        home.join("server.log").display(),
        log_tail(home)
    )
}

pub async fn service(action: Action) -> Result<i32> {
    let address = match discovery::choose(nonempty)? {
        Chosen::Home(home) => {
            return local_service(action, &home).await;
        }
        Chosen::At(address) => address,
    };
    anyhow::ensure!(
        action != Action::Stop,
        "Cannot stop HAMLET_SERVER here. A server elsewhere is \
         stopped on its own machine."
    );
    if action == Action::Status {
        let live = is_answering(&address).await;
        println!(
            "hamlet: {} at {}",
            if live { "running" } else { "not answering" },
            address.url
        );
        return Ok(i32::from(!live));
    }
    open_detached(&address)?;
    Ok(0)
}

#[cfg(unix)]
async fn local_service(action: Action, home: &Path) -> Result<i32> {
    let found = inspect(home).await?;
    if let (Action::Start(_), Some((record, _))) = (action, &found)
        && record.confinement == Some(Confinement::Insecure)
    {
        println!("Attached to a server in insecure mode.");
    }
    let found = match (action, found) {
        (Action::Start(isolation), None) => {
            Some(start(home, isolation).await?)
        }
        (_, found) => found,
    };
    let Some((record, address)) = found else {
        let stale = running(home)?.is_some();
        if stale && action == Action::Stop {
            fs::remove_file(home.join("server.json"))?;
        }
        println!(
            "hamlet: {}not running",
            if stale { "stale server.json; " } else { "" }
        );
        return Ok(i32::from(action != Action::Stop));
    };
    if action == Action::Stop {
        signal(record.pid, Signal::TERM)?;
        let deadline = Instant::now() + Duration::from_secs(30);
        while is_alive(record.pid) {
            anyhow::ensure!(
                Instant::now() < deadline,
                "Process {} is still stopping after 30 seconds. \
                 Inspect {}; no SIGKILL was sent.",
                record.pid,
                home.join("server.log").display()
            );
            sleep(Duration::from_millis(100)).await;
        }
        println!("hamlet: stopped");
    } else {
        println!(
            "hamlet: running at {}; pid {}; started {}; log {}",
            record.url,
            record.pid,
            record.started,
            home.join("server.log").display()
        );
        if let Action::Start(_) = action {
            open_detached(&address)?;
        }
    }
    Ok(0)
}

#[cfg(unix)]
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

#[cfg(unix)]
fn dev_home(chosen: Option<&str>) -> Result<PathBuf> {
    let uid = rustix::process::getuid().as_raw();
    let path = chosen
        .map(PathBuf::from)
        .or_else(|| nonempty("HAMLET_HOME").map(PathBuf::from))
        .unwrap_or_else(|| {
            env::temp_dir().join(format!("hamlet-dev-{uid}"))
        });
    let path = std::path::absolute(&path)?;
    let ordinary = PathBuf::from(env::var("HOME")?).join(".hamlet");
    anyhow::ensure!(
        chosen != Some("~/.hamlet")
            && canonical(&path)? != canonical(&ordinary)?,
        "hamlet dev refuses ~/.hamlet; choose a scratch home with \
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
    } else if stat.uid() != uid {
        Some("not owned by the current user")
    } else if stat.mode() & 0o022 != 0 {
        Some("writable by group or others")
    } else {
        None
    };
    if let Some(reason) = unsafe_reason {
        bail!("unsafe scratch home {}: {reason}.", path.display());
    }
    Ok(path)
}

#[cfg(unix)]
async fn dev_binary(name: &str, variable: &str) -> Result<PathBuf> {
    if let Some(path) = nonempty(variable) {
        return Ok(path.into());
    }
    let root = repo();
    anyhow::ensure!(
        root.join("Cargo.toml").is_file(),
        "hamlet dev needs a source checkout or {variable}."
    );
    let mut command = tokio::process::Command::new("cargo");
    command
        .current_dir(&root)
        .args([
            "build",
            "--bin",
            name,
            "--message-format=json-render-diagnostics",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    let output = command.output().await.with_context(|| {
        format!("Cannot build {name}; install Rust and Cargo")
    })?;
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

#[cfg(unix)]
async fn wait_child(child: &mut Child) -> Result<()> {
    timeout(Duration::from_secs(30), child.wait())
        .await
        .context(
            "Child is still stopping after 30 seconds; no SIGKILL \
             was sent",
        )??;
    Ok(())
}

/// The address of the dev server `server` once it answers there.
#[cfg(unix)]
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
#[cfg(unix)]
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

#[cfg(unix)]
pub async fn develop(
    port: u16,
    faux: Option<&str>,
    chosen: Option<&str>,
    isolation: Isolation,
) -> Result<i32> {
    use tokio::signal::unix::{SignalKind, signal as listen};
    let home = dev_home(chosen)?;
    let server =
        dev_binary("hamlet_server", "HAMLET_SERVER_BIN").await?;
    let app = dev_binary("hamlet_app", "HAMLET_APP").await?;
    let mut interrupt = listen(SignalKind::interrupt())?;
    let mut terminate = listen(SignalKind::terminate())?;
    let mut hangup = listen(SignalKind::hangup())?;
    let mut command =
        server_command(&server, &home, port, faux, isolation);
    session(&mut command);
    command.stdin(Stdio::null());
    let mut server =
        tokio::process::Command::from(command).spawn()?;
    let mut window = None;
    let result = async {
        let ready = ready(&mut server, &home);
        let ready = timeout(Duration::from_secs(15), ready);
        let address = tokio::select! {
            ready = ready => ready.context(
                "The server did not become ready within 15 seconds",
            )??,
            _ = interrupt.recv() => return Ok(0),
            _ = terminate.recv() => return Ok(0),
            _ = hangup.recv() => return Ok(0),
        };
        let mut command = window_command(&app, &address);
        command.env("HAMLET_VIEW", home.join("window"));
        session(&mut command);
        let child = tokio::process::Command::from(command)
            .spawn()
            .with_context(|| {
                format!("Cannot open {}", app.display())
            })?;
        window = Some(child);
        loop {
            tokio::select! {
                _ = interrupt.recv() => return Ok(0),
                _ = terminate.recv() => return Ok(0),
                _ = hangup.recv() => return Ok(0),
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
    for pid in [window.as_ref().and_then(Child::id), server.id()]
        .into_iter()
        .flatten()
    {
        signal(pid, Signal::TERM)?;
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
    let force = async {
        tokio::select! {
            _ = interrupt.recv() => {},
            _ = terminate.recv() => {},
            _ = hangup.recv() => {},
        }
    };
    tokio::select! {
        closed = closing => closed?,
        _ = force => {
            let window_pid = window.as_ref().and_then(Child::id);
            let pids = [window_pid, server.id()];
            for pid in pids.into_iter().flatten() {
                signal(pid, Signal::KILL)?;
            }
            if let Some(child) = &mut window {
                child.wait().await?;
            }
            server.wait().await?;
            return Ok(130);
        }
    }
    result
}

#[cfg(not(unix))]
pub fn serve(_: u16, _: Option<&str>, _: Isolation) -> Result<i32> {
    bail!(
        "The agent server requires macOS or Linux. Use \
         HAMLET_SERVER and HAMLET_TOKEN to connect to another \
         machine."
    )
}

#[cfg(not(unix))]
async fn local_service(_: Action, _: &Path) -> Result<i32> {
    serve(0, None, Isolation::Insecure)
}

#[cfg(not(unix))]
pub async fn develop(
    _: u16,
    _: Option<&str>,
    _: Option<&str>,
    isolation: Isolation,
) -> Result<i32> {
    serve(0, None, isolation)
}
