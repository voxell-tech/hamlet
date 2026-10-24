//! Authenticated operations of each home's agents, confined to one
//! house and its optional shared project.

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result, bail};
use rustix::fs::{FlockOperation, flock};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixListener;
use tokio::process::Command;
use tokio::sync::watch;
use tokio::task::JoinSet;

use crate::wire::{
    Code, LIBRARY_FILES, Library, MAX_BODY, MAX_FILE, Message,
    Progress, Reply, Request, State, is_valid_id, is_valid_name,
};
use crate::{http, proxy};

/// What a journal marker holds when its operation was cancelled
/// before it was admitted. An admitted operation's marker is empty.
const CANCELLED: &[u8] = b"cancelled";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Configuration {
    listen: String,
    root: PathBuf,
    /// Each home's name and the bearer token its Hamlet sends.
    homes: HashMap<String, String>,
}

pub async fn run(config: Configuration) -> Result<()> {
    let homes = tokens(&config.homes)?;
    // A missing root is a missing volume, never a new workspace.
    let root = fs::canonicalize(&config.root).with_context(|| {
        format!(
            "The worker's root {} is missing",
            config.root.display()
        )
    })?;
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
    // A dot keeps the control root apart from every valid home name.
    let control = root.join(".control");
    fs::create_dir_all(&control)?;
    fs::set_permissions(&control, fs::Permissions::from_mode(0o700))?;
    let lock = fs::File::create(control.join("lock"))?;
    // The lock is held while this event loop runs.
    if flock(&lock, FlockOperation::NonBlockingLockExclusive).is_err()
    {
        bail!("Worker data is already open");
    }
    let network = control.join("network.sock");
    if fs::symlink_metadata(&network).is_ok() {
        fs::remove_file(&network)?;
    }
    let listener = UnixListener::bind(&network)?;
    fs::set_permissions(&network, fs::Permissions::from_mode(0o600))?;
    let proxy = proxy::serve(listener);
    tokio::pin!(proxy);
    let mut calls = http::listen(&config.listen).await?;
    let mut worker = Worker {
        root,
        homes,
        network,
        running: HashMap::new(),
        tasks: JoinSet::new(),
    };
    loop {
        tokio::select! {
            served = &mut proxy => {
                served?;
                bail!("The network proxy stopped");
            }
            Some(completed) = worker.tasks.join_next(),
                if !worker.tasks.is_empty() =>
            {
                if let Ok(key) = completed {
                    worker.running.remove(&key);
                }
            }
            Some(call) = calls.recv() => worker.on_call(call)?,
            else => break,
        }
    }
    Ok(())
}

/// What the worker's loop holds between calls.
struct Worker {
    root: PathBuf,
    /// Each home by [`bearer`] of its authorization.
    homes: HashMap<[u8; 32], String>,
    network: PathBuf,
    /// The cancel switch of each running operation, by home and id.
    running: HashMap<String, watch::Sender<bool>>,
    /// The running operations, each ending with its key in
    /// `running`.
    tasks: JoinSet<String>,
}

impl Worker {
    /// Answers one authenticated call, or starts the operation that
    /// will.
    fn on_call(&mut self, call: http::Call) -> Result<()> {
        let (home_name, message) = match self.admitted(&call) {
            Ok(admitted) => admitted,
            Err(refused) => {
                let _ = call.reply.send(refused);
                return Ok(());
            }
        };
        let home = self.root.join(&home_name);
        let journal = home.join("operations");
        fs::create_dir_all(&journal)?;
        let id = message.id().to_owned();
        let marker = journal.join(&id);
        let result = journal.join(format!("{id}.json"));
        let active_key = format!("{home_name}:{id}");
        match message {
            Message::Cancel { .. } => {
                let state = if let Some(cancel) =
                    self.running.get(&active_key)
                {
                    let _ = cancel.send(true);
                    Some(State::Cancelling)
                } else if is_cancelled(&marker)
                    || mark(&journal, &marker, CANCELLED).is_ok()
                {
                    // Marked before this reply, so the id is never
                    // admitted, even after a restart.
                    Some(State::Cancelled)
                } else {
                    marker.exists().then_some(State::Completed)
                };
                let _ = call.reply.send(
                    state.map_or_else(Reply::uncertain, |state| {
                        Reply::ok(Progress { state })
                    }),
                );
            }
            Message::Execute { request } => {
                if request.version != 1
                    || !is_valid_name(&request.workspace)
                {
                    let _ = call.reply.send(Reply::error(
                        Code::NotSupported,
                        "This workspace service supports protocol 1 \
                         only.",
                        false,
                    ));
                    return Ok(());
                }
                if self.running.len() >= 16 {
                    let _ = call.reply.send(Reply::error(
                        Code::Unavailable,
                        "The worker is at its operation limit. Wait \
                         for existing commands to finish.",
                        false,
                    ));
                    return Ok(());
                }
                // An id is admitted once; a repeated one is never run
                // again.
                if mark(&journal, &marker, &[]).is_err() {
                    let _ =
                        call.reply.send(if is_cancelled(&marker) {
                            Reply::error(
                                Code::Aborted,
                                "Workspace operation cancelled \
                                 before admission. Nothing was run.",
                                false,
                            )
                        } else {
                            Reply::uncertain()
                        });
                    return Ok(());
                }
                let (cancel, cancelled) = watch::channel(false);
                self.running.insert(active_key.clone(), cancel);
                let network = self.network.clone();
                self.tasks.spawn(async move {
                    let reply =
                        execute(&home, &network, request, cancelled)
                            .await
                            .unwrap_or_else(|_| {
                                Reply::error(
                                    Code::Confinement,
                                    "The confined worker did not \
                                     establish a completed result. \
                                     Inspect the operation before \
                                     repeating it; no local \
                                     fallback was used.",
                                    true,
                                )
                            });
                    let reply = if save(&result, &reply).is_ok() {
                        reply
                    } else {
                        Reply::uncertain()
                    };
                    let _ = call.reply.send(reply);
                    active_key
                });
            }
        }
        Ok(())
    }

    /// The home and message of `call` when its token is a home's,
    /// else the reply that refuses it.
    fn admitted(
        &self,
        call: &http::Call,
    ) -> Result<(String, Message), Reply> {
        let home = self
            .homes
            .get(&bearer(&call.authorization))
            .ok_or_else(|| {
                Reply::error(
                    Code::PermissionDenied,
                    "Workspace authentication failed. Ask the chief \
                     to check the connection.",
                    false,
                )
            })?;
        match serde_json::from_slice::<Message>(&call.body) {
            Ok(message) if is_valid_id(message.id()) => {
                Ok((home.clone(), message))
            }
            _ => Err(Reply::error(
                Code::Invalid,
                "Invalid workspace request.",
                false,
            )),
        }
    }
}

/// Each home by the digest of the authorization header its Hamlet
/// sends, so a token is never compared byte by byte.
fn tokens(
    homes: &HashMap<String, String>,
) -> Result<HashMap<[u8; 32], String>> {
    if homes.is_empty() {
        bail!("The worker serves no home");
    }
    let mut tokens = HashMap::new();
    for (home, token) in homes {
        if !is_valid_name(home) || token.len() < 16 {
            bail!("Invalid name or token of home {home:?}");
        }
        if tokens
            .insert(bearer(&format!("Bearer {token}")), home.clone())
            .is_some()
        {
            bail!("Two homes share a token");
        }
    }
    Ok(tokens)
}

fn bearer(authorization: &str) -> [u8; 32] {
    Sha256::digest(authorization).into()
}

/// Creates the journal's `marker` holding `contents`, durably. Fails
/// when it already exists.
fn mark(
    journal: &Path,
    marker: &Path,
    contents: &[u8],
) -> std::io::Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(marker)?;
    file.write_all(contents)?;
    file.sync_all()?;
    fs::File::open(journal)?.sync_all()
}

/// Whether `marker`'s operation was cancelled before admission.
fn is_cancelled(marker: &Path) -> bool {
    fs::read(marker).is_ok_and(|contents| contents == CANCELLED)
}

/// Replaces the file at `path` with `reply`, durably.
fn save(path: &Path, reply: &Reply) -> Result<()> {
    let temporary = path.with_extension("pending");
    let mut file = fs::File::create(&temporary)?;
    file.write_all(&serde_json::to_vec(reply)?)?;
    file.sync_all()?;
    fs::rename(temporary, path)?;
    fs::File::open(path.parent().context("Journal has no folder")?)?
        .sync_all()?;
    Ok(())
}

/// Runs one operation under a trusted PID 1 in fresh namespaces. Only
/// a network-granted operation gets loopback and the proxy socket.
async fn execute(
    home: &Path,
    network: &Path,
    request: Request,
    mut cancelled: watch::Receiver<bool>,
) -> Result<Reply> {
    let workspace = home.join(&request.workspace);
    let house =
        workspace.join("houses").join(request.agent.to_string());
    fs::create_dir_all(&house)?;
    if fs::symlink_metadata(&house)?.is_symlink() {
        bail!("House root is a symlink");
    }
    let library = match library(home, &request) {
        Ok(library) => library,
        Err(Missing) => {
            return Ok(Reply::error(
                Code::LibraryMissing,
                "The workspace does not hold the named library. \
                 Send its files again.",
                false,
            ));
        }
    };
    let executable = std::env::current_exe()?;
    let mut command = Command::new(&executable);
    command.arg("netless");
    if request.network {
        command.arg("--loopback");
    }
    command.arg("/usr/bin/bwrap").args([
        "--unshare-all",
        "--share-net",
        "--die-with-parent",
        "--new-session",
        "--as-pid-1",
        "--cap-drop",
        "ALL",
        "--clearenv",
    ]);
    for path in ["/usr", "/bin", "/lib", "/lib64"] {
        if Path::new(path).exists() {
            command.args(["--ro-bind", path, path]);
        }
    }
    command
        .args(["--proc", "/proc", "--dev", "/dev", "--tmpfs", "/tmp"])
        .arg(if request.read_only {
            "--ro-bind"
        } else {
            "--bind"
        })
        .arg(&house)
        .arg("/house")
        .arg("--ro-bind")
        .arg(executable)
        .arg("/runner");
    if let Some(library) = &library {
        command.arg("--ro-bind").arg(library).arg("/library");
    }
    if request.project {
        let project = workspace.join("project");
        fs::create_dir_all(&project)?;
        if fs::symlink_metadata(&project)?.is_symlink() {
            bail!("Project root is a symlink");
        }
        command
            .arg(if request.read_only {
                "--ro-bind"
            } else {
                "--bind"
            })
            .arg(project)
            .arg("/project");
    }
    if request.network {
        command.arg("--ro-bind").arg(network).arg("/network.sock");
        // Expose the public trust bundle, never the rest of /etc.
        command.args([
            "--ro-bind",
            "/etc/ssl/certs/ca-certificates.crt",
            "/etc/ssl/certs/ca-certificates.crt",
        ]);
    }
    command
        .args([
            "--chdir",
            "/house",
            "--remount-ro",
            "/",
            "/runner",
            "init",
        ])
        .args(request.network.then_some("network"))
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command.spawn()?;
    let mut input =
        child.stdin.take().context("Missing supervisor input")?;
    let mut output = child
        .stdout
        .take()
        .context("Missing supervisor output")?
        .take((MAX_BODY + 1) as u64);
    let mut errors = child
        .stderr
        .take()
        .context("Missing supervisor errors")?
        .take(8192);
    input
        .write_all(&serde_json::to_vec(&request.operation)?)
        .await?;
    input.write_all(b"\n").await?;
    let receive = async {
        let mut bytes = Vec::new();
        let mut diagnostics = Vec::new();
        tokio::try_join!(
            output.read_to_end(&mut bytes),
            errors.read_to_end(&mut diagnostics)
        )?;
        if !child.wait().await?.success() || bytes.len() > MAX_BODY {
            bail!("Supervisor failed");
        }
        Ok::<_, anyhow::Error>(serde_json::from_slice::<Reply>(
            &bytes,
        )?)
    };
    tokio::pin!(receive);
    tokio::select! {
        reply = &mut receive => return reply,
        _ = cancelled.changed() => {}
        _ = tokio::time::sleep(Duration::from_secs(135)) => {}
    }
    drop(input);
    tokio::time::timeout(Duration::from_secs(10), &mut receive)
        .await?
}

/// How long a library snapshot nobody used is kept.
const LIBRARY_KEPT: Duration = Duration::from_secs(15 * 60);

/// A request named a library the worker does not hold.
struct Missing;

/// The read-only shelf snapshot a request names, kept under
/// `.library/<sha256>` and written there once from the files that
/// came with it.
fn library(
    home: &Path,
    request: &Request,
) -> Result<Option<PathBuf>, Missing> {
    let Some(library) = &request.library else {
        return Ok(None);
    };
    let parent = home.join(".library");
    let named = library.sha256.len() == 64
        && library.sha256.bytes().all(|b| b.is_ascii_hexdigit());
    let root = parent.join(&library.sha256);
    // Touched on each use, so a snapshot in use is never swept.
    if named
        && fs::symlink_metadata(&root).is_ok_and(|m| m.is_dir())
        && fs::File::open(&root)
            .and_then(|dir| dir.set_modified(SystemTime::now()))
            .is_ok()
    {
        return Ok(Some(root));
    }
    let Some(files) = library.files.as_ref().filter(|_| named) else {
        return Err(Missing);
    };
    match store(&parent, &request.id, library, files) {
        Ok(()) => Ok(Some(root)),
        Err(_) => Err(Missing),
    }
}

/// Writes the snapshot of `files` to `parent/<sha256>` and sweeps the
/// snapshots unused for [`LIBRARY_KEPT`].
fn store(
    parent: &Path,
    id: &str,
    library: &Library,
    files: &[crate::wire::LibraryFile],
) -> Result<()> {
    if files.len() > LIBRARY_FILES
        || files.iter().map(|file| file.data.0.len()).sum::<usize>()
            > MAX_FILE
        || Library::digest(files) != library.sha256
    {
        bail!("Invalid library");
    }
    fs::create_dir_all(parent)?;
    let incoming = parent.join(format!(".incoming-{id}"));
    fs::create_dir(&incoming)?;
    let written = (|| {
        for entry in files {
            if entry.path.split('/').any(|part| {
                part.is_empty() || part == "." || part == ".."
            }) || entry.mode & !0o777 != 0
            {
                bail!("Invalid library path or mode");
            }
            let path = incoming.join(&entry.path);
            fs::create_dir_all(
                path.parent().context("Missing library parent")?,
            )?;
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            file.write_all(&entry.data.0)?;
            fs::set_permissions(
                &path,
                fs::Permissions::from_mode(entry.mode & 0o555),
            )?;
        }
        Ok(())
    })();
    // Another request may have stored the same snapshot first.
    if written.is_err()
        || fs::rename(&incoming, parent.join(&library.sha256))
            .is_err()
    {
        fs::remove_dir_all(&incoming)?;
    }
    written?;
    for entry in fs::read_dir(parent)?.flatten() {
        let stale =
            entry.metadata().and_then(|m| m.modified()).is_ok_and(
                |at| at.elapsed().is_ok_and(|age| age > LIBRARY_KEPT),
            );
        if stale && entry.file_name() != library.sha256.as_str() {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
    Ok(())
}
