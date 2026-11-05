//! A realm working in a remote workspace, served by a loopback
//! double of the worker that keeps its files in a scratch folder: the
//! agents' tools write their remote lodge, the server brings it home,
//! and a path outside the workspace is refused there.

use std::io::{Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;
use std::{fs, thread};

use hamlet_acceptance::{Client, Faux, call, chat, words};
use hamlet_protocol::client::{
    ClientMessage, Edit, EditSettingsSet, EditSettingsSetWorkspace,
    Keeper, MoveLodges, ReplyKind,
};
use hamlet_testing::{Scratch, Server};
use hamlet_workspace_wire::{self as wire, Operation};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::Realm;

const TOKEN: &str = "offline-workspace-token-123456";

/// The worker's double, serving until dropped.
struct Worker {
    files: Scratch,
    url: String,
    received: mpsc::Receiver<Value>,
    stop: mpsc::Sender<()>,
    serving: Option<thread::JoinHandle<()>>,
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        let ended = self.serving.take().unwrap().join();
        if !thread::panicking() {
            ended.unwrap();
        }
    }
}

impl Worker {
    fn start() -> Self {
        let files = Scratch::new("remote-worker");
        let root = files.to_path_buf();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url =
            format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let (sent, received) = mpsc::channel();
        let (stop, stopped) = mpsc::channel();
        let serving = thread::spawn(move || {
            while stopped.try_recv().is_err() {
                let Ok((mut stream, _)) = listener.accept() else {
                    thread::sleep(Duration::from_millis(2));
                    continue;
                };
                let message = read(&mut stream);
                sent.send(message.clone()).unwrap();
                let reply = if message["kind"] == "cancel" {
                    json!({"kind": "ok", "value": {"state": "completed"}})
                } else {
                    performed(&root, &message["request"])
                };
                answer(&mut stream, &reply);
            }
        });
        Self {
            files,
            url,
            received,
            stop,
            serving: Some(serving),
        }
    }

    /// The requests the worker was sent so far.
    fn requests(&self) -> Vec<Value> {
        self.received
            .try_iter()
            .filter_map(|message| {
                (message["kind"] == "execute")
                    .then(|| message["request"].clone())
            })
            .collect()
    }
}

/// One request's body, checked as the worker checks its head.
fn read(stream: &mut TcpStream) -> Value {
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut head = Vec::new();
    while !head.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        stream.read_exact(&mut byte).unwrap();
        head.push(byte[0]);
    }
    let head = String::from_utf8(head).unwrap().to_lowercase();
    assert!(head.starts_with("post /v1 http/1.1\r\n"), "{head}");
    assert!(head.contains(&format!("authorization: bearer {TOKEN}")));
    let length = head
        .lines()
        .find_map(|line| line.strip_prefix("content-length: "))
        .unwrap()
        .parse::<usize>()
        .unwrap();
    let mut body = vec![0; length];
    stream.read_exact(&mut body).unwrap();
    serde_json::from_slice(&body).unwrap()
}

fn answer(stream: &mut TcpStream, reply: &Value) {
    let body = reply.to_string();
    let _ = write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: \
         application/json\r\nContent-Length: {}\r\nConnection: \
         close\r\n\r\n{body}",
        body.len()
    );
}

fn performed(root: &Path, request: &Value) -> Value {
    let request =
        serde_json::from_value::<wire::Request>(request.clone())
            .expect("the server sends typed workspace requests");
    let reply = match perform(root, &request) {
        Ok(value) => wire::Reply::Ok { value },
        Err(code) => wire::Reply::error(code, "refused", false),
    };
    serde_json::to_value(reply).unwrap()
}

/// Where `path` is in the worker's files: in the agent's lodge, its
/// workspace's project or the library, and nowhere else.
fn located(
    root: &Path,
    request: &wire::Request,
    path: &str,
) -> Result<PathBuf, wire::Code> {
    if path.split('/').any(|part| part == "..") {
        return Err(wire::Code::PermissionDenied);
    }
    let workspace = root.join(&request.workspace);
    for (prefix, dir) in [
        (
            "/lodge",
            workspace.join(format!("lodges/{}", request.agent)),
        ),
        ("/project", workspace.join("project")),
        ("/library", workspace.join("library")),
    ] {
        if path == prefix {
            return Ok(dir);
        }
        if let Some(rest) = path.strip_prefix(&format!("{prefix}/")) {
            return Ok(dir.join(rest));
        }
    }
    Err(wire::Code::PermissionDenied)
}

fn code(error: std::io::Error) -> wire::Code {
    match error.kind() {
        std::io::ErrorKind::NotFound => wire::Code::NotFound,
        std::io::ErrorKind::AlreadyExists => {
            wire::Code::AlreadyExists
        }
        _ => wire::Code::Invalid,
    }
}

fn stat(path: &Path) -> std::io::Result<wire::Stat> {
    let info = fs::symlink_metadata(path)?;
    Ok(wire::Stat {
        name: path.file_name().unwrap().to_string_lossy().into(),
        path: path.to_string_lossy().into(),
        kind: if info.is_dir() {
            wire::Kind::Directory
        } else {
            wire::Kind::File
        },
        size: info.len(),
        mtime_ms: 0,
    })
}

fn tree(
    root: &Path,
    path: &Path,
    entries: &mut Vec<wire::TreeEntry>,
) -> std::io::Result<()> {
    let info = fs::symlink_metadata(path)?;
    let relative = path.strip_prefix(root).unwrap().to_str().unwrap();
    let kind = if info.is_dir() {
        wire::TreeKind::Directory
    } else {
        wire::TreeKind::File
    };
    let file = kind == wire::TreeKind::File;
    entries.push(wire::TreeEntry {
        path: if relative.is_empty() { "." } else { relative }.into(),
        kind,
        mode: info.permissions().mode() & 0o777,
        size: if file { info.len() } else { 0 },
        sha256: file
            .then(|| {
                fs::read(path).map(|bytes| {
                    format!("{:x}", Sha256::digest(bytes))
                })
            })
            .transpose()?,
    });
    if !file {
        for entry in fs::read_dir(path)? {
            tree(root, &entry?.path(), entries)?;
        }
    }
    Ok(())
}

/// Takes the request's library as the worker does: by its digest,
/// with its files only when the worker does not hold it yet.
fn library(
    root: &Path,
    request: &wire::Request,
) -> Result<(), wire::Code> {
    let Some(library) = &request.library else {
        return Ok(());
    };
    let held = root.join(".library").join(&library.sha256);
    let Some(files) = &library.files else {
        return if held.exists() {
            Ok(())
        } else {
            Err(wire::Code::LibraryMissing)
        };
    };
    assert_eq!(wire::Library::digest(files), library.sha256);
    let shelf = located(root, request, "/library")?;
    for file in files {
        let path = shelf.join(&file.path);
        fs::create_dir_all(path.parent().unwrap()).map_err(code)?;
        fs::write(path, &file.data.0).map_err(code)?;
    }
    fs::create_dir_all(held).map_err(code)
}

fn ok(value: impl serde::Serialize) -> Result<Value, wire::Code> {
    Ok(serde_json::to_value(value).unwrap())
}

/// `request` carried out on the worker's files. A command is not run:
/// it answers with fixed output.
fn perform(
    root: &Path,
    request: &wire::Request,
) -> Result<Value, wire::Code> {
    let lodge = located(root, request, "/lodge")?;
    fs::create_dir_all(&lodge).map_err(code)?;
    library(root, request)?;
    let at = |path: &str| located(root, request, path);
    match request.operation.clone() {
        Operation::InitLodge(_) => {
            for name in ["memory", "skills", "inbox"] {
                fs::create_dir_all(lodge.join(name)).map_err(code)?;
            }
            ok(())
        }
        Operation::Info(info) => {
            ok(stat(&at(&info.path)?).map_err(code)?)
        }
        Operation::Canonical(canonical) => ok(canonical.path),
        Operation::List(list) => ok(fs::read_dir(at(&list.path)?)
            .map_err(code)?
            .map(|entry| stat(&entry?.path()))
            .collect::<std::io::Result<Vec<_>>>()
            .map_err(code)?),
        Operation::Read(read) => {
            let path = at(&read.path)?;
            ok(wire::Contents {
                info: stat(&path).map_err(code)?,
                data: wire::Bytes(fs::read(&path).map_err(code)?),
            })
        }
        Operation::Write(write) => {
            let path = at(&write.path)?;
            if write.parents {
                fs::create_dir_all(path.parent().unwrap())
                    .map_err(code)?;
            }
            fs::OpenOptions::new()
                .write(true)
                .create(true)
                .append(write.append)
                .truncate(!write.append)
                .open(&path)
                .and_then(|mut file| file.write_all(&write.data.0))
                .map_err(code)?;
            ok(())
        }
        Operation::TreeWrite(write) => {
            let path = at(&write.path)?;
            fs::create_dir_all(path.parent().unwrap())
                .map_err(code)?;
            fs::write(&path, &write.data.0).map_err(code)?;
            fs::set_permissions(
                &path,
                fs::Permissions::from_mode(write.mode),
            )
            .map_err(code)?;
            ok(())
        }
        Operation::SetMode(set) => {
            fs::set_permissions(
                at(&set.path)?,
                fs::Permissions::from_mode(set.mode),
            )
            .map_err(code)?;
            ok(())
        }
        Operation::Mkdir(mkdir) => {
            fs::create_dir_all(at(&mkdir.path)?).map_err(code)?;
            ok(())
        }
        Operation::TreeList(list) => {
            let path = at(&list.path)?;
            let mut entries = Vec::new();
            tree(&path, &path, &mut entries).map_err(code)?;
            ok(entries)
        }
        Operation::Remove(remove) => {
            let path = at(&remove.path)?;
            let removed = if path.is_dir() {
                fs::remove_dir_all(path)
            } else {
                fs::remove_file(path)
            };
            match removed {
                Err(error)
                    if remove.force
                        && error.kind()
                            == std::io::ErrorKind::NotFound => {}
                removed => removed.map_err(code)?,
            }
            ok(())
        }
        Operation::Walk(walk) => {
            let base = at(&walk.path)?.to_string_lossy().into_owned();
            ok(wire::walk(
                &walk.path,
                &walk.skip,
                walk.files,
                |dir| {
                    let physical = PathBuf::from(format!(
                        "{base}{}",
                        &dir[walk.path.len()..]
                    ));
                    fs::read_dir(physical).map(|entries| {
                        entries
                            .flatten()
                            .filter_map(|entry| {
                                let info = entry.metadata().ok()?;
                                let kind = if info.is_dir() {
                                    wire::Listed::Directory
                                } else {
                                    wire::Listed::File(info.len())
                                };
                                Some((
                                    entry
                                        .file_name()
                                        .to_string_lossy()
                                        .into(),
                                    kind,
                                ))
                            })
                            .collect()
                    })
                },
            ))
        }
        Operation::Exec(_) => ok(wire::Ran {
            exit_code: 0,
            stdout: "remote-command-output".into(),
            stderr: String::new(),
        }),
    }
}

/// A realm whose chief chose `worker` and moved the lodges there,
/// served again so that it works there.
async fn moved_to(
    worker: &Worker,
    faux: Faux,
) -> (Realm, Server, Client) {
    let mut realm = Realm::new();
    let (server, mut chief) = realm.serve(Faux::new()).await;
    // What the Hand learned before, which the move must carry.
    let hand = chief.hand();
    let reply = chief
        .request(|id| {
            ClientMessage::Edit(Edit::Memory {
                id,
                of: Keeper::Agent(hand),
                path: "tea.md".into(),
                text: "---\ndescription: Tea\n---\nEarl grey.\n"
                    .into(),
            })
        })
        .await;
    assert_eq!(reply.kind, ReplyKind::Notice, "{}", reply.text);
    let url = worker.url.clone();
    let reply = chief
        .request(|id| {
            ClientMessage::Edit(Edit::Settings {
                id,
                set: EditSettingsSet {
                    allow_net: None,
                    character: None,
                    effort: None,
                    endpoints: None,
                    model: None,
                    newcomers: None,
                    reflect_every: None,
                    workspace: Some(Some(EditSettingsSetWorkspace {
                        token: TOKEN.into(),
                        url,
                    })),
                },
            })
        })
        .await;
    assert_eq!(reply.kind, ReplyKind::Notice, "{}", reply.text);
    let reply = chief
        .request(|id| ClientMessage::MoveLodges(MoveLodges { id }))
        .await;
    assert_eq!(reply.kind, ReplyKind::Notice, "{}", reply.text);
    drop((server, chief));
    let (server, chief) = realm.serve(faux).await;
    (realm, server, chief)
}

#[tokio::test]
async fn the_hands_tools_write_its_remote_lodge_and_the_server_brings_it_home()
 {
    let worker = Worker::start();
    let write = call(
        "write",
        json!({"path": "memory/remote.md", "content": "Kept remotely.\n"}),
    );
    let host = call("read", json!({"path": "/etc/hosts"}));
    let (realm, _server, mut chief) = moved_to(
        &worker,
        Faux::new().responses([write, host, words("Written.")]),
    )
    .await;
    let tea = worker
        .files
        .join("realm/lodges")
        .join(format!("{}/memory/tea.md", chief.hand().0));
    assert!(tea.exists(), "the lodges were moved");

    let hand = chief.hand();
    let hand_chat = chat(hand);
    chief.open(&hand_chat).await;
    chief
        .tell(hand, "Remember this, then read the hosts.")
        .await;
    chief.shows(&hand_chat, "Written.").await;

    // The write ran remotely and came home beside what was kept.
    let home = realm.path("hand/memory/remote.md");
    assert_eq!(fs::read_to_string(home).unwrap(), "Kept remotely.\n");
    assert!(realm.path("hand/memory/tea.md").exists());
    // A host path is refused there, never read here.
    let tools = chief
        .items(&hand_chat)
        .into_iter()
        .filter_map(|item| item.tool)
        .collect::<Vec<_>>();
    assert_eq!(tools.len(), 2, "{tools:?}");
    assert_eq!(tools[0].failure, None, "{tools:?}");
    assert!(tools[1].failure.is_some(), "{tools:?}");
    let requests = worker.requests();
    // The library goes over once, then by its digest alone.
    let carried = requests
        .iter()
        .filter(|r| r["library"]["files"].is_array())
        .count();
    assert_eq!(carried, 1, "{requests:#?}");
}
