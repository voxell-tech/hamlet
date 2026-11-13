//! The web IDE on a hamlet's folder: one OpenVSCode Server for each
//! folder, started when a window first asks for it, under the
//! sandbox with the folder alone to write. It listens on this
//! machine only and takes a token, which the address it gives holds.

use std::collections::BTreeMap;
use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::Result;
use hamlet_os::fs::path::text;
use hamlet_os::process::{Command, Limits, Tree};
use hamlet_sandbox::{Env, Policy, Sandbox, host_env};
use hamlet_store::ids::HamletId;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::net::TcpStream;
use tokio::task::AbortHandle;
use tokio::time::{Instant, sleep};
use url::Url;

use crate::core::Core;
use crate::society::{authority, hamlets};

/// The variable that names the `openvscode-server` script to run.
pub(crate) const PROGRAM: &str = "HAMLET_WEB_IDE";

/// The variable that names a folder of extensions to give every web
/// IDE, such as the viewers for PDF and slides.
pub(crate) const EXTENSIONS: &str = "HAMLET_WEB_IDE_EXTENSIONS";

/// The settings every web IDE starts with: an explorer and an
/// editor, without the chat, git, debug and the like. They go where
/// the server keeps its machine's settings, since the browser keeps
/// the user's own.
const SETTINGS: &str = include_str!("web_ide/settings.json");

/// How long a server may take to listen after it starts.
const LISTEN_WITHIN: Duration = Duration::from_secs(20);

/// The web IDE servers that run, by the folder each is on.
#[derive(Default)]
pub(crate) struct WebIdes {
    running: BTreeMap<PathBuf, Running>,
}

struct Running {
    port: u16,
    token: String,
    /// Cleared when the server's processes end.
    alive: Arc<AtomicBool>,
    /// The task that owns the server's processes, which end with it.
    task: AbortHandle,
    /// Kept while the server runs: its private temp folder, which
    /// holds its settings.
    _env: Env,
}

impl Drop for Running {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl WebIdes {
    /// The port and token of the server on `folder`, if one runs.
    fn alive(&mut self, folder: &Path) -> Option<(u16, String)> {
        let running = self.running.get(folder)?;
        if running.alive.load(Ordering::Relaxed) {
            return Some((running.port, running.token.clone()));
        }
        self.running.remove(folder);
        None
    }
}

/// How long a server may use the CPU: as long as it runs.
const FOREVER: u64 = 10 * 365 * 24 * 3600;

/// Reads `output` to its end and drops it, so the server never
/// waits on a full pipe.
async fn drain(mut output: impl AsyncRead + Unpin) {
    let mut buffer = vec![0; 16 * 1024];
    while matches!(output.read(&mut buffer).await, Ok(n) if n > 0) {}
}

/// The address of a web IDE on `hamlet`'s folder, started if it does
/// not run; or why there is none.
pub(crate) async fn address(
    core: &mut Core,
    hamlet: &HamletId,
) -> Result<Result<String, String>> {
    let folder = match folder_of(core, hamlet)? {
        Ok(folder) => folder,
        Err(words) => return Ok(Err(words)),
    };
    if let Some((port, token)) = core.web_ides.alive(&folder) {
        return Ok(Ok(open_address(port, &token, &folder)));
    }
    let Some(program) = std::env::var_os(PROGRAM) else {
        return Ok(Err(format!(
            "No web IDE is set up: {PROGRAM} names no \
             openvscode-server."
        )));
    };
    let started = match start(
        &core.sandbox,
        Path::new(&program),
        &folder,
        core.realm.layout().root(),
    ) {
        Ok(started) => started,
        Err(words) => return Ok(Err(words)),
    };
    let (port, token) = (started.port, started.token.clone());
    core.web_ides.running.insert(folder.clone(), started);
    if !listening(port).await {
        core.web_ides.running.remove(&folder);
        return Ok(
            Err("The web IDE did not start listening.".into()),
        );
    }
    Ok(Ok(open_address(port, &token, &folder)))
}

/// The folder of `hamlet`, or why this chief cannot have a web IDE
/// on it.
fn folder_of(
    core: &mut Core,
    hamlet: &HamletId,
) -> Result<Result<PathBuf, String>> {
    if core.workspace.is_some() {
        return Ok(Err("The web IDE runs on this machine, and the \
                       agents work in a remote workspace."
            .into()));
    }
    let chief = core.realm.chief();
    let db = core.realm.store().connection();
    let found =
        hamlets::find(db, hamlet)?.filter(hamlets::Hamlet::is_living);
    let Some(found) = found else {
        return Ok(Err(format!("There is no hamlet `{hamlet}`.")));
    };
    if !authority::is_chief_of(db, chief, hamlet)? {
        return Ok(Err(format!(
            "The hamlet `{hamlet}` is not yours."
        )));
    }
    Ok(found.folder.map(PathBuf::from).ok_or_else(|| {
        format!("The hamlet `{hamlet}` works on no folder.")
    }))
}

/// Starts `program` on `folder`, confined to write it alone and to
/// read all but `realm`.
fn start(
    sandbox: &Sandbox,
    program: &Path,
    folder: &Path,
    realm: &Path,
) -> Result<Running, String> {
    let port = free_port().map_err(|error| {
        format!("No port is free for the web IDE: {error}")
    })?;
    let token = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    // It listens on this machine's own network, which a sandbox
    // without one would keep from the window.
    let policy = Policy {
        rw: vec![folder.to_owned()],
        open: true,
        hidden: vec![realm.to_owned()],
        env: host_env(),
        net: true,
        ..Policy::default()
    };
    let env = Env::new(&text(folder), policy, *sandbox).map_err(
        |error| format!("The sandbox could not be set up: {error}"),
    )?;
    let tmp = PathBuf::from(env.tmp());
    // In a file, so the token is not in the process list.
    let token_file = tmp.join("token");
    std::fs::write(&token_file, &token).map_err(|error| {
        format!("The web IDE's token could not be written: {error}")
    })?;
    let (data, extensions) =
        (tmp.join("data"), tmp.join("extensions"));
    prepare(&data, &extensions).map_err(|error| {
        format!(
            "The web IDE's settings could not be written: {error}"
        )
    })?;
    let confined = env.confine(&arguments(
        program,
        port,
        &token_file,
        &data,
        &extensions,
    ));
    let (mut tree, stdout, stderr) = Tree::spawn(
        Command {
            argv: &confined.argv,
            env: &confined.env,
            cwd: env.cwd(),
            stdin: Stdio::null(),
        },
        &Limits {
            cpu_secs: FOREVER,
            wall_secs: FOREVER,
            ..Limits::default()
        },
    )
    .map_err(|error| {
        format!("The web IDE could not start: {error}")
    })?;
    let alive = Arc::new(AtomicBool::new(true));
    let ended = alive.clone();
    let task = tokio::spawn(async move {
        tokio::join!(drain(stdout), drain(stderr), async {
            let _ = tree.ended().await;
            ended.store(false, Ordering::Relaxed);
        });
    })
    .abort_handle();
    Ok(Running {
        port,
        token,
        alive,
        task,
        _env: env,
    })
}

/// Writes the settings into `data` and copies the
/// extensions to give into `extensions`, which the server may write.
fn prepare(data: &Path, extensions: &Path) -> std::io::Result<()> {
    let machine = data.join("Machine");
    std::fs::create_dir_all(&machine)?;
    std::fs::write(machine.join("settings.json"), SETTINGS)?;
    std::fs::create_dir_all(extensions)?;
    if let Some(from) = std::env::var_os(EXTENSIONS) {
        copy_all(Path::new(&from), extensions)?;
    }
    Ok(())
}

/// Copies the files and folders in `from` into `to`. The server's
/// list of extensions holds their places, which it is given anew.
fn copy_all(from: &Path, to: &Path) -> std::io::Result<()> {
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            std::fs::create_dir_all(&target)?;
            copy_all(&entry.path(), &target)?;
        } else if entry.file_name() == "extensions.json" {
            let list = std::fs::read_to_string(entry.path())?;
            std::fs::write(
                target,
                list.replace(&text(from), &text(to)),
            )?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// The command line of a server on `port`.
fn arguments(
    program: &Path,
    port: u16,
    token_file: &Path,
    data: &Path,
    extensions: &Path,
) -> Vec<String> {
    vec![
        text(program),
        "--host".into(),
        "127.0.0.1".into(),
        "--port".into(),
        port.to_string(),
        "--connection-token-file".into(),
        text(token_file),
        "--user-data-dir".into(),
        text(data),
        "--extensions-dir".into(),
        text(extensions),
        "--telemetry-level".into(),
        "off".into(),
        "--accept-server-license-terms".into(),
    ]
}

/// A port nothing listens on now.
fn free_port() -> std::io::Result<u16> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    Ok(listener.local_addr()?.port())
}

/// Whether something listens on `port` within [`LISTEN_WITHIN`].
async fn listening(port: u16) -> bool {
    let until = Instant::now() + LISTEN_WITHIN;
    let at = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    while Instant::now() < until {
        if TcpStream::connect(at).await.is_ok() {
            return true;
        }
        sleep(Duration::from_millis(200)).await;
    }
    false
}

/// The address of the server on `port` with `token`, opened on
/// `folder`.
fn open_address(port: u16, token: &str, folder: &Path) -> String {
    let mut address =
        Url::parse(&format!("http://127.0.0.1:{port}/"))
            .expect("a loopback address");
    address
        .query_pairs_mut()
        .append_pair("tkn", token)
        .append_pair("folder", &text(folder));
    address.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_address_holds_the_token_and_the_folder_encoded() {
        let address =
            open_address(3000, "abc", Path::new("/tmp/my folder"));
        assert_eq!(
            address,
            "http://127.0.0.1:3000/?tkn=abc&folder=%2Ftmp%2Fmy+folder"
        );
    }

    #[test]
    fn the_token_is_given_in_a_file_and_never_on_the_command_line() {
        let words = arguments(
            Path::new("/opt/code/bin/openvscode-server"),
            3000,
            Path::new("/tmp/t/token"),
            Path::new("/tmp/t/data"),
            Path::new("/tmp/t/extensions"),
        );
        assert!(
            words.contains(&"--connection-token-file".to_owned())
        );
        assert!(!words.contains(&"--connection-token".to_owned()));
        assert_eq!(
            words[1..5],
            ["--host", "127.0.0.1", "--port", "3000"]
        );
    }
}
