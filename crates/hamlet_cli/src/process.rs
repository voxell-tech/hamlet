//! Local server discovery and the lifetimes of the server and window.

#![expect(
    unsafe_code,
    reason = "on Unix a session's leader is made between fork and \
              exec, and on Windows a process is opened by its ID"
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
use hamlet_protocol::client::Confinement;
use hamlet_protocol::client::discovery::{self, Chosen, Running};
#[cfg(unix)]
use rustix::io::Errno;
#[cfg(unix)]
use rustix::process::{Pid, Signal, kill_process, test_kill_process};
use tokio::process::Child;
use tokio::time::{Instant, sleep, timeout};

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
         home's token; it is not ours to touch. Inspect it on its \
         machine; no signal was sent.",
        record.pid,
        home.display()
    )
}

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
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
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

#[cfg(windows)]
fn session(command: &mut Command) {
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
    } else {
        shared(&stat)
    };
    if let Some(reason) = unsafe_reason {
        bail!("unsafe scratch home {}: {reason}.", path.display());
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

pub async fn develop(
    port: u16,
    faux: Option<&str>,
    chosen: Option<&str>,
    isolation: Isolation,
) -> Result<i32> {
    let home = dev_home(chosen)?;
    let server =
        dev_binary("hamlet_server", "HAMLET_SERVER_BIN").await?;
    let app = dev_binary("hamlet_app", "HAMLET_APP").await?;
    let mut stops = Stops::new()?;
    let mut command =
        server_command(&server, &home, port, faux, isolation);
    // The server is kept from the terminal's Ctrl-C, and this
    // process asks it to stop.
    #[cfg(unix)]
    session(&mut command);
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(
        &mut command,
        CREATE_NEW_PROCESS_GROUP,
    );
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
            () = stops.next() => return Ok(0),
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
