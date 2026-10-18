//! Signed actor operations confined to one house and its optional
//! shared project.

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ring::signature::{ED25519, UnparsedPublicKey};
use serde::Deserialize;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixListener;
use tokio::process::Command;
use tokio::sync::watch;
use tokio::task::JoinSet;

use crate::control::save;
use crate::wire::{
    Envelope, MAX_BODY, MAX_FILE, Message, Reply, Request, valid_id,
    valid_name,
};
use crate::{http, proxy};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Configuration {
    listen: String,
    root: PathBuf,
    public_key: String,
    actor_uid_file: PathBuf,
}

pub async fn run(config: Configuration) -> Result<()> {
    fs::create_dir_all(&config.root)?;
    let root = fs::canonicalize(config.root)?;
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
    let lock = fs::File::create(root.join("lock"))?;
    // SAFETY: the owned lock stays open while this event loop runs.
    if unsafe {
        libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB)
    } != 0
    {
        bail!("Worker data is already open");
    }
    let key = STANDARD.decode(config.public_key)?;
    if key.len() != 32 {
        bail!("Invalid worker verification key");
    }
    let key = UnparsedPublicKey::new(&ED25519, key);
    // A dot keeps the control root apart from every valid home name.
    let control = root.join(".control");
    fs::create_dir_all(&control)?;
    fs::set_permissions(&control, fs::Permissions::from_mode(0o700))?;
    let network = control.join("network.sock");
    if fs::symlink_metadata(&network).is_ok() {
        fs::remove_file(&network)?;
    }
    let listener = UnixListener::bind(&network)?;
    fs::set_permissions(&network, fs::Permissions::from_mode(0o600))?;
    let proxy = proxy::serve(listener);
    tokio::pin!(proxy);
    let mut calls = http::listen(&config.listen).await?;
    let mut running = HashMap::<String, watch::Sender<bool>>::new();
    let mut tasks = JoinSet::new();
    loop {
        tokio::select! {
            served = &mut proxy => {
                served?;
                bail!("The network proxy stopped");
            }
            Some(completed) = tasks.join_next(), if !tasks.is_empty() => {
                if let Ok(id) = completed { running.remove(&id); }
            }
            Some(call) = calls.recv() => {
                let fresh = call.timestamp.parse::<u64>().ok().is_some_and(|seconds| SystemTime::now().duration_since(UNIX_EPOCH).is_ok_and(|now| now.as_secs().abs_diff(seconds) <= 180));
                let signed = [call.timestamp.as_bytes(), b"\n", &call.body].concat();
                let signature = STANDARD.decode(&call.signature).unwrap_or_default();
                if !fresh || key.verify(&signed, &signature).is_err() {
                    let _ = call.reply.send(Reply::error("permission_denied", "Invalid workspace signature.", false)); continue;
                }
                let envelope = match serde_json::from_slice::<Envelope>(&call.body) {
                    Ok(value) if valid_name(&value.home) && valid_id(value.message.id()) => value,
                    _ => { let _ = call.reply.send(Reply::error("invalid", "Invalid signed workspace request.", false)); continue; }
                };
                // SystemInfo is refreshed on restore; memory may still hold the snapshot source's UID.
                if envelope.actor_uid.is_empty() || !fs::read_to_string(&config.actor_uid_file).is_ok_and(|uid| uid == envelope.actor_uid) {
                    let _ = call.reply.send(Reply::error("permission_denied", "Workspace actor identity does not match.", false)); continue;
                }
                let home = root.join(&envelope.home);
                let journal = home.join("operations");
                fs::create_dir_all(&journal)?;
                let id = envelope.message.id().to_owned();
                let marker = journal.join(&id);
                let result = journal.join(format!("{id}.json"));
                let active_key = format!("{}:{id}", envelope.home);
                match envelope.message {
                    Message::Status { .. } => {
                        let reply = if running.contains_key(&active_key) { Reply::ok(json!({ "state": "running" })) }
                            else { fs::read(&result).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_else(Reply::uncertain) };
                        let _ = call.reply.send(reply);
                    }
                    Message::Cancel { .. } => {
                        if let Some(cancel) = running.get(&active_key) { let _ = cancel.send(true); }
                        else if !marker.exists() { fs::File::create(&marker)?.sync_all()?; }
                        let _ = call.reply.send(Reply::ok(json!({ "state": "cancelling" })));
                    }
                    Message::Execute { request } => {
                        if request.version != 1 || !valid_name(&request.workspace) {
                            let _ = call.reply.send(Reply::error("not_supported", "Only protocol 1 is supported.", false)); continue;
                        }
                        if running.len() >= 16 {
                            let _ = call.reply.send(Reply::error("unavailable", "The worker is at its operation limit. Wait for existing commands to finish.", false)); continue;
                        }
                        match fs::OpenOptions::new().write(true).create_new(true).open(marker) {
                            Ok(file) => { file.sync_all()?; fs::File::open(&journal)?.sync_all()?; }
                            Err(_) => { let _ = call.reply.send(Reply::uncertain()); continue; }
                        }
                        let (cancel, cancelled) = watch::channel(false);
                        running.insert(active_key.clone(), cancel);
                        let network = network.clone();
                        tasks.spawn(async move {
                            let reply = execute(&home, &network, request, cancelled).await.unwrap_or_else(|_| Reply::error("confinement", "The confined worker did not establish a completed result. Inspect the operation before repeating it; no local fallback was used.", true));
                            let reply = if save(&result, &reply).is_ok() { reply } else { Reply::uncertain() };
                            let _ = call.reply.send(reply);
                            active_key
                        });
                    }
                }
            }
            else => break,
        }
    }
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
    let library = library(home, &request)?;
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
        command.arg("--ro-bind").arg(&library.0).arg("/library");
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
        reply = &mut receive => reply,
        _ = cancelled.changed() => {
            drop(input);
            tokio::time::timeout(Duration::from_secs(10), &mut receive).await?
        }
        _ = tokio::time::sleep(Duration::from_secs(135)) => {
            drop(input);
            tokio::time::timeout(Duration::from_secs(10), &mut receive).await?
        }
    }
}

/// The operation's read-only shelf snapshot, removed after its
/// process tree exits.
struct Library(PathBuf);

impl Drop for Library {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn library(
    home: &Path,
    request: &Request,
) -> Result<Option<Library>> {
    let Some(files) = &request.library else {
        return Ok(None);
    };
    if files.len() > 1024 {
        bail!("Too many library files");
    }
    let parent = home.join(".library");
    fs::create_dir_all(&parent)?;
    let root = parent.join(&request.id);
    fs::create_dir(&root)?;
    let library = Library(root);
    let mut bytes = 0;
    for entry in files {
        if entry.path.split('/').any(|part| {
            part.is_empty() || part == "." || part == ".."
        }) || entry.mode & !0o777 != 0
        {
            bail!("Invalid library path or mode");
        }
        let data = STANDARD.decode(&entry.data)?;
        bytes += data.len();
        if bytes > MAX_FILE {
            bail!("Library exceeds 4 MiB");
        }
        let path = library.0.join(&entry.path);
        fs::create_dir_all(
            path.parent().context("Missing library parent")?,
        )?;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        file.write_all(&data)?;
        fs::set_permissions(
            &path,
            fs::Permissions::from_mode(entry.mode & 0o555),
        )?;
    }
    Ok(Some(library))
}
