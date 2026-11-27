//! Local server discovery and the lifetimes of the server and window.

#![cfg_attr(
    windows,
    expect(
        unsafe_code,
        reason = "on Windows a process is opened by its ID"
    )
)]

use std::io::{Read, Seek, SeekFrom};
#[cfg(unix)]
use std::os::unix::{
    fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    process::{CommandExt, ExitStatusExt},
};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::Duration;
use std::{env, fs};

use anyhow::{Context, Result, bail};
#[cfg(unix)]
use hamlet_os::process::own_session;
use hamlet_protocol::client::Confinement;
use hamlet_protocol::client::discovery::{self, Chosen, Running};
#[cfg(unix)]
use rustix::io::Errno;
#[cfg(unix)]
use rustix::process::{Pid, Signal, kill_process, test_kill_process};
use tokio::time::{Instant, sleep, timeout};

use crate::client::{self, ServerAddress};

mod dev;

pub use dev::develop;

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

/// A server started for a home that never became ready. It reads as
/// the words for a terminal; `reason` and `log` are for a dialog.
#[derive(Debug)]
pub struct NotReady {
    message: String,
    /// The server's own last error, from its log.
    pub reason: Option<String>,
    pub log: PathBuf,
}

impl std::fmt::Display for NotReady {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for NotReady {}

impl NotReady {
    fn new(home: &Path, why: &str) -> Self {
        let log = home.join("server.log");
        let tail = log_tail(home);
        // The server ends its log with `Error: ` and why it stopped,
        // its causes on the lines after.
        let reason = tail
            .rfind("Error: ")
            .filter(|&at| at == 0 || tail[..at].ends_with('\n'))
            .map(|at| tail[at + "Error: ".len()..].to_owned());
        Self {
            message: format!(
                "Cannot start the server: {why}. Inspect {} and \
                 hamlet status before trying again.\n{tail}",
                log.display()
            ),
            reason,
            log,
        }
    }
}

fn nonempty(name: &str) -> Option<String> {
    env::var(name).ok().filter(|s| !s.is_empty())
}

/// The local home the environment selects.
fn home() -> Result<PathBuf> {
    Ok(discovery::home(nonempty)?)
}

/// The server the environment selects.
pub fn address() -> Result<ServerAddress> {
    Ok(ServerAddress::find(nonempty)?)
}

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

#[cfg(windows)]
mod kernel32 {
    use std::ffi::c_void;
    use std::io;
    use std::os::windows::io::{
        AsRawHandle, FromRawHandle, OwnedHandle,
    };

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(
            access: u32,
            inherit: i32,
            pid: u32,
        ) -> *mut c_void;
        fn WaitForSingleObject(handle: *mut c_void, ms: u32) -> u32;
        fn TerminateProcess(handle: *mut c_void, code: u32) -> i32;
        fn GenerateConsoleCtrlEvent(event: u32, group: u32) -> i32;
    }

    const CTRL_BREAK_EVENT: u32 = 1;

    const PROCESS_TERMINATE: u32 = 0x0001;
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const SYNCHRONIZE: u32 = 0x0010_0000;
    const WAIT_TIMEOUT: u32 = 0x0102;
    const ERROR_ACCESS_DENIED: i32 = 5;

    fn open(pid: u32, access: u32) -> io::Result<OwnedHandle> {
        // SAFETY: the call reads only its arguments.
        let handle = unsafe { OpenProcess(access, 0, pid) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the handle was just opened, and nothing else owns
        // it.
        Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
    }

    pub(super) fn is_alive(pid: u32) -> bool {
        match open(
            pid,
            SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
        ) {
            Ok(process) => {
                // SAFETY: the handle is open while `process` lives.
                let state = unsafe {
                    WaitForSingleObject(process.as_raw_handle(), 0)
                };
                state == WAIT_TIMEOUT
            }
            // Denied says the process exists but is not ours.
            Err(error) => {
                error.raw_os_error() == Some(ERROR_ACCESS_DENIED)
            }
        }
    }

    /// Sends Ctrl-Break to the process group `pid` leads on this
    /// console.
    pub(super) fn interrupt(pid: u32) -> io::Result<()> {
        // SAFETY: the call reads only its arguments.
        if unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, pid) }
            == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub(super) fn terminate(pid: u32) -> io::Result<()> {
        let process = open(pid, PROCESS_TERMINATE)?;
        // SAFETY: the handle is open while `process` lives.
        if unsafe { TerminateProcess(process.as_raw_handle(), 1) }
            == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

#[cfg(windows)]
use kernel32::is_alive;

/// Asks the server `pid` of `home` to stop, and waits until it has.
#[cfg(unix)]
async fn stop(pid: u32, home: &Path) -> Result<()> {
    signal(pid, Signal::TERM)?;
    wait_gone(pid, home, "no SIGKILL was sent").await
}

/// Ends the server `pid` of `home`. Windows has no signal another
/// console can send a detached process, so it is terminated, and the
/// kill-on-close job it put itself in ends pi and every command with
/// it. Its record is removed for it.
#[cfg(windows)]
async fn stop(pid: u32, home: &Path) -> Result<()> {
    kernel32::terminate(pid)?;
    wait_gone(pid, home, "it was terminated").await?;
    match fs::remove_file(home.join("server.json")) {
        Err(error)
            if error.kind() != std::io::ErrorKind::NotFound =>
        {
            Err(error.into())
        }
        _ => Ok(()),
    }
}

async fn wait_gone(pid: u32, home: &Path, sent: &str) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(30);
    while is_alive(pid) {
        anyhow::ensure!(
            Instant::now() < deadline,
            "Process {pid} is still stopping after 30 seconds. \
             Inspect {}; {sent}.",
            home.join("server.log").display()
        );
        sleep(Duration::from_millis(100)).await;
    }
    Ok(())
}

/// What stops `hamlet dev` and `hamlet server`: Ctrl-C and its kin.
struct Stops {
    #[cfg(unix)]
    signals: [tokio::signal::unix::Signal; 3],
    #[cfg(windows)]
    interrupt: tokio::signal::windows::CtrlC,
    #[cfg(windows)]
    brk: tokio::signal::windows::CtrlBreak,
    #[cfg(windows)]
    close: tokio::signal::windows::CtrlClose,
}

impl Stops {
    fn new() -> Result<Self> {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{SignalKind, signal};
            Ok(Self {
                signals: [
                    signal(SignalKind::interrupt())?,
                    signal(SignalKind::terminate())?,
                    signal(SignalKind::hangup())?,
                ],
            })
        }
        #[cfg(windows)]
        {
            use tokio::signal::windows::{
                ctrl_break, ctrl_c, ctrl_close,
            };
            Ok(Self {
                interrupt: ctrl_c()?,
                brk: ctrl_break()?,
                close: ctrl_close()?,
            })
        }
    }

    async fn next(&mut self) {
        #[cfg(unix)]
        {
            let [interrupt, terminate, hangup] = &mut self.signals;
            tokio::select! {
                _ = interrupt.recv() => {}
                _ = terminate.recv() => {}
                _ = hangup.recv() => {}
            }
        }
        #[cfg(windows)]
        {
            tokio::select! {
                _ = self.interrupt.recv() => {}
                _ = self.brk.recv() => {}
                _ = self.close.recv() => {}
            }
        }
    }
}

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
         realm's token; it is not ours to touch. Inspect it on its \
         machine; no signal was sent.",
        record.pid,
        home.display()
    )
}

/// The checkout, as a plain path: Bevy's file watcher strips it from
/// the paths it is told of, which never hold a `..`. Two folders up
/// from this crate, which Cargo names in full; with its links
/// resolved where it can be found.
fn repo() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root =
        manifest.parent().and_then(Path::parent).unwrap_or(manifest);
    root.canonicalize().unwrap_or_else(|_| root.to_path_buf())
}

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

/// Serves in the foreground. The server shares this console, so
/// Ctrl-C reaches it, and this process outlives it to give its exit
/// code.
#[cfg(windows)]
pub fn serve(
    port: u16,
    faux: Option<&str>,
    isolation: Isolation,
) -> Result<i32> {
    let server = find_binary("hamlet_server", "HAMLET_SERVER_BIN")?;
    let command =
        server_command(&server, &home()?, port, faux, isolation);
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let mut stops = Stops::new()?;
            let mut server = tokio::process::Command::from(command)
                .spawn()
                .context(
                    "Cannot start hamlet_server; build it or set \
                     HAMLET_SERVER_BIN",
                )?;
            loop {
                tokio::select! {
                    status = server.wait() => {
                        return Ok(exit_code(status?));
                    }
                    () = stops.next() => {}
                }
            }
        })
}

#[cfg(windows)]
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

#[cfg(windows)]
fn own_session(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    command
        .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
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

/// Opens a window on `address` in its own session, and leaves it.
pub fn open_detached(address: &ServerAddress) -> Result<()> {
    let opened = (|| -> Result<()> {
        let mut command = window_command(
            &find_binary("hamlet_app", "HAMLET_APP")?,
            address,
        );
        own_session(&mut command);
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

fn mkdir(home: &Path) -> Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    builder.mode(0o700);
    builder.create(home)?;
    Ok(())
}

/// Starts a server for `home`, or finds the one another `hamlet`
/// started meanwhile.
async fn start(
    home: &Path,
    isolation: Isolation,
) -> Result<(Running, ServerAddress)> {
    discovery::readable(home)?;
    mkdir(home)?;
    let mut log = fs::OpenOptions::new();
    log.create(true).append(true);
    #[cfg(unix)]
    log.mode(0o600);
    let log = log.open(home.join("server.log"))?;
    let log_start = log.metadata()?.len();
    let server = find_binary("hamlet_server", "HAMLET_SERVER_BIN")?;
    let mut command =
        server_command(&server, home, 0, None, isolation);
    own_session(&mut command);
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
                return Err(NotReady::new(
                    home,
                    "it exited before becoming ready",
                )
                .into());
            }
        }
        sleep(Duration::from_millis(100)).await;
    }
    Err(NotReady::new(
        home,
        "it did not become ready within 15 seconds",
    )
    .into())
}

/// The server the environment selects: the local home's, started
/// unless it serves, or the one `HAMLET_SERVER` names.
pub async fn ensure(isolation: Isolation) -> Result<ServerAddress> {
    match discovery::choose(nonempty)? {
        Chosen::At(address) => Ok(address),
        Chosen::Home(home) => Ok(match inspect(&home).await? {
            Some((_, address)) => address,
            None => start(&home, isolation).await?.1,
        }),
    }
}

/// Stops the local home's server, if one serves, whichever server
/// the environment otherwise selects.
pub async fn stop_home() -> Result<()> {
    let home = home()?;
    if let Some((record, _)) = inspect(&home).await? {
        stop(record.pid, &home).await?;
    }
    Ok(())
}

/// The window, ready to open on `address`.
pub fn window_for(address: &ServerAddress) -> Result<Command> {
    Ok(window_command(
        &find_binary("hamlet_app", "HAMLET_APP")?,
        address,
    ))
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
        stop(record.pid, home).await?;
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
