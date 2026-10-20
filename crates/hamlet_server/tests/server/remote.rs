//! Public server requests against an offline workspace-v1 manager
//! double.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::{fs, thread};

use hamlet_acceptance::{Script, call, words};
use hamlet_workspace_wire as wire;
use sha2::{Digest, Sha256};

use super::*;

const TOKEN: &str = "offline-workspace-token-123456";
type Socket = tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
>;

struct Manager {
    files: Scratch,
    url: String,
    received: mpsc::Receiver<Value>,
    stop: mpsc::Sender<()>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Drop for Manager {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        let result = self.worker.take().unwrap().join();
        if !thread::panicking() {
            result.unwrap();
        }
    }
}

impl Manager {
    fn start() -> Self {
        let files = Scratch::new(&format!(
            "remote-manager-{}",
            uuid::Uuid::new_v4()
        ));
        let root = files.to_path_buf();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url =
            format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let (sent, received) = mpsc::channel();
        let (stop, stopped) = mpsc::channel();
        let worker = thread::spawn(move || {
            let mut held = Vec::<(String, TcpStream)>::new();
            let mut paused =
                Vec::<(TcpStream, Value, Option<Value>)>::new();
            while stopped.try_recv().is_err() {
                if !root.join("hold-request").exists() {
                    for (mut stream, request, result) in
                        paused.drain(..)
                    {
                        let reply = result.unwrap_or_else(|| {
                            performed(&root, &request)
                        });
                        answer(&mut stream, reply);
                    }
                }
                let Ok((mut stream, _)) = listener.accept() else {
                    thread::sleep(Duration::from_millis(2));
                    continue;
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut header = Vec::new();
                while !header.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    if stream.read_exact(&mut byte).is_err() {
                        break;
                    }
                    header.push(byte[0]);
                }
                let header = String::from_utf8(header).unwrap();
                assert!(
                    header.starts_with("POST /v1 HTTP/1.1\r\n"),
                    "{header}"
                );
                assert!(header.to_lowercase().contains(&format!(
                    "authorization: bearer {TOKEN}"
                )));
                let length = header
                    .lines()
                    .find_map(|line| {
                        line.to_lowercase()
                            .strip_prefix("content-length: ")
                            .map(|length| {
                                length.parse::<usize>().unwrap()
                            })
                    })
                    .unwrap();
                assert!(length <= 12 * 1024 * 1024);
                let mut body = vec![0; length];
                stream.read_exact(&mut body).unwrap();
                let message =
                    serde_json::from_slice::<Value>(&body).unwrap();
                sent.send(message.clone()).unwrap();
                if message["kind"] == "cancel" {
                    if let Some(index) = held
                        .iter()
                        .position(|(id, _)| message["id"] == *id)
                    {
                        let (_, mut waiting) = held.remove(index);
                        answer(
                            &mut waiting,
                            json!({"kind":"error","code":"aborted","message":"Canceled remote request","uncertain":true}),
                        );
                    }
                    answer(
                        &mut stream,
                        json!({"kind":"ok","value":null}),
                    );
                    continue;
                }
                let request = &message["request"];
                assert_eq!(request["version"], 1);
                uuid::Uuid::parse_str(
                    request["id"].as_str().unwrap(),
                )
                .unwrap();
                if let Ok(hold) = fs::read(root.join("hold-request"))
                    && let Ok(hold) =
                        serde_json::from_slice::<Value>(&hold)
                    && hold.as_object().unwrap().iter().all(
                        |(key, value)| {
                            request["operation"][key] == *value
                        },
                    )
                {
                    let result = (request["operation"]["kind"]
                        == "write")
                        .then(|| performed(&root, request));
                    // Control requests must progress without the
                    // manager's help, even after a write took effect.
                    paused.push((stream, request.clone(), result));
                    continue;
                }
                if let Ok(failure) =
                    fs::read(root.join("fail-request"))
                    && let Ok(failure) =
                        serde_json::from_slice::<Value>(&failure)
                    && failure["operation"]
                        .as_object()
                        .unwrap()
                        .iter()
                        .all(|(key, value)| {
                            request["operation"][key] == *value
                        })
                {
                    if failure["uncertain"] == true {
                        perform(&root, request).unwrap();
                    }
                    answer(
                        &mut stream,
                        json!({"kind":"error","code":"permission_denied","message":"controlled manager refusal","uncertain":failure["uncertain"]}),
                    );
                    continue;
                }
                if request["operation"]["argv"][2] == "hold-remote" {
                    held.push((
                        request["id"].as_str().unwrap().into(),
                        stream,
                    ));
                    continue;
                }
                if request["operation"]["path"] == "/house/uncertain"
                    && request["operation"]["kind"] == "write"
                {
                    perform(&root, request).unwrap();
                    answer(
                        &mut stream,
                        json!({"kind":"error","code":"uncertain","message":format!("Lost reply {TOKEN}"),"uncertain":true}),
                    );
                    continue;
                }
                answer(&mut stream, performed(&root, request));
            }
        });
        Self {
            files,
            url,
            received,
            stop,
            worker: Some(worker),
        }
    }

    fn house(&self, number: u64) -> PathBuf {
        self.files.join(format!("home/{number}"))
    }
}

/// The manager's reply to `request`, which must be a typed one.
fn performed(root: &Path, request: &Value) -> Value {
    let reply = match perform(root, request) {
        Ok(value) => wire::Reply::Ok { value },
        Err(code) => {
            wire::Reply::error(code, "manager refused path", false)
        }
    };
    serde_json::to_value(reply).unwrap()
}

fn answer(stream: &mut TcpStream, value: Value) {
    let body = value.to_string();
    let _ = write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: \
         application/json\r\nContent-Length: {}\r\nConnection: \
         close\r\n\r\n{body}",
        body.len()
    );
}

fn location(
    root: &Path,
    request: &wire::Request,
    path: &str,
) -> Result<PathBuf, wire::Code> {
    if path.split('/').any(|part| part == "..") {
        return Err(wire::Code::PermissionDenied);
    }
    let workspace = &request.workspace;
    let agent = request.agent;
    for (prefix, dir) in [
        ("/house", root.join(format!("{workspace}/{agent}"))),
        ("/project", root.join(format!("{workspace}/project"))),
        ("/library", root.join(format!("{workspace}/library"))),
    ] {
        let rest = if path == prefix {
            ""
        } else if let Some(rest) =
            path.strip_prefix(&format!("{prefix}/"))
        {
            rest
        } else {
            continue;
        };
        return Ok(dir.join(rest));
    }
    Err(wire::Code::PermissionDenied)
}

/// `path`, refused as the worker refuses it where `no_follow` is set
/// and a link is anywhere in it.
fn reached(
    root: &Path,
    request: &wire::Request,
    path: &str,
    no_follow: bool,
) -> Result<PathBuf, wire::Code> {
    let at = location(root, request, path)?;
    if no_follow {
        let top = path.split('/').nth(1).unwrap_or_default();
        let mut current =
            location(root, request, &format!("/{top}"))?;
        let rest = at.strip_prefix(&current).unwrap().to_path_buf();
        for part in rest.components() {
            current.push(part);
            if fs::symlink_metadata(&current)
                .is_ok_and(|info| info.is_symlink())
            {
                return Err(wire::Code::Invalid);
            }
        }
    }
    Ok(at)
}

fn code(error: std::io::Error) -> wire::Code {
    match error.kind() {
        std::io::ErrorKind::NotFound => wire::Code::NotFound,
        std::io::ErrorKind::AlreadyExists => {
            wire::Code::AlreadyExists
        }
        std::io::ErrorKind::PermissionDenied => {
            wire::Code::PermissionDenied
        }
        _ => wire::Code::Invalid,
    }
}

fn stat(path: &Path) -> std::io::Result<wire::Stat> {
    let info = fs::symlink_metadata(path)?;
    Ok(wire::Stat {
        name: path.file_name().unwrap().to_string_lossy().into(),
        path: path.to_string_lossy().into(),
        kind: if info.is_symlink() {
            wire::Kind::Symlink
        } else if info.is_dir() {
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
    let kind = if info.is_symlink() {
        wire::TreeKind::Link
    } else if info.is_dir() {
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
    if kind == wire::TreeKind::Directory {
        for entry in fs::read_dir(path)? {
            tree(root, &entry?.path(), entries)?;
        }
    }
    Ok(())
}

/// Takes the request's library as the worker does: by its digest,
/// with its files only where this manager does not hold them yet.
fn library(
    root: &Path,
    request: &wire::Request,
) -> Result<(), wire::Code> {
    let Some(library) = &request.library else {
        return Ok(());
    };
    let held = root.join(".libraries").join(&library.sha256);
    let Some(files) = &library.files else {
        return if held.exists() {
            Ok(())
        } else {
            Err(wire::Code::LibraryMissing)
        };
    };
    assert_eq!(wire::Library::digest(files), library.sha256);
    let shelf = location(root, request, "/library")?;
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

/// `request` carried out on the manager's files, as protocol v1
/// types it.
fn perform(
    root: &Path,
    request: &Value,
) -> Result<Value, wire::Code> {
    let request =
        serde_json::from_value::<wire::Request>(request.clone())
            .expect("the server sends typed workspace requests");
    let house = location(root, &request, "/house")?;
    fs::create_dir_all(&house).map_err(code)?;
    library(root, &request)?;
    let at = |path: &str, no_follow| {
        reached(root, &request, path, no_follow)
    };
    use wire::Operation as Op;
    let writes = matches!(
        request.operation,
        Op::Write(_)
            | Op::TreeWrite(_)
            | Op::Remove(_)
            | Op::Mkdir(_)
            | Op::SetMode(_)
    );
    if request.read_only && writes {
        return Err(wire::Code::PermissionDenied);
    }
    match request.operation.clone() {
        Op::InitHouse(_) => {
            fs::create_dir_all(house.join("skills")).map_err(code)?;
            fs::create_dir_all(house.join("inbox")).map_err(code)?;
            if !house.join("NOTES.md").exists() {
                fs::write(house.join("NOTES.md"), "# Notes\n")
                    .map_err(code)?;
            }
            ok(())
        }
        Op::Info(info) => {
            ok(stat(&at(&info.path, info.no_follow)?)
                .map_err(code)?)
        }
        Op::Canonical(canonical) => ok(canonical.path),
        Op::List(list) => {
            ok(fs::read_dir(at(&list.path, list.no_follow)?)
                .map_err(code)?
                .map(|entry| stat(&entry?.path()))
                .collect::<std::io::Result<Vec<_>>>()
                .map_err(code)?)
        }
        Op::Read(read) => {
            let path = at(&read.path, read.no_follow)?;
            if read.no_follow
                && fs::symlink_metadata(&path)
                    .map_err(code)?
                    .is_symlink()
            {
                return Err(wire::Code::Invalid);
            }
            ok(wire::Contents {
                info: stat(&path).map_err(code)?,
                data: wire::Bytes(fs::read(&path).map_err(code)?),
            })
        }
        Op::Write(write) => {
            let path = at(&write.path, write.no_follow)?;
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
        Op::TreeWrite(write) => {
            let path = at(&write.path, true)?;
            fs::create_dir_all(path.parent().unwrap())
                .map_err(code)?;
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .and_then(|mut file| file.write_all(&write.data.0))
                .and_then(|()| {
                    fs::set_permissions(
                        &path,
                        fs::Permissions::from_mode(write.mode),
                    )
                })
                .map_err(code)?;
            ok(())
        }
        Op::SetMode(set) => {
            let path = at(&set.path, true)?;
            if fs::symlink_metadata(&path).map_err(code)?.is_symlink()
            {
                return Err(wire::Code::Invalid);
            }
            fs::set_permissions(
                path,
                fs::Permissions::from_mode(set.mode),
            )
            .map_err(code)?;
            ok(())
        }
        Op::Mkdir(mkdir) => {
            let path = at(&mkdir.path, mkdir.no_follow)?;
            if mkdir.recursive {
                fs::create_dir_all(path)
            } else {
                fs::create_dir(path)
            }
            .map_err(code)?;
            ok(())
        }
        Op::TreeList(list) => {
            let path = at(&list.path, true)?;
            let mut entries = Vec::new();
            tree(&path, &path, &mut entries).map_err(code)?;
            if entries.len() as u64 > list.entries {
                return Err(wire::Code::TooMany);
            }
            ok(entries)
        }
        Op::Remove(remove) => {
            let path = at(&remove.path, remove.no_follow)?;
            let removed = match fs::symlink_metadata(&path) {
                Ok(info) if info.is_dir() && remove.recursive => {
                    fs::remove_dir_all(path)
                }
                Ok(info) if info.is_dir() => fs::remove_dir(path),
                Ok(_) => fs::remove_file(path),
                Err(error) => Err(error),
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
        Op::Walk(walk) => {
            let root = at(&walk.path, true)?;
            let base = root.to_string_lossy().into_owned();
            let walked = wire::walk(
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
                                let info = fs::symlink_metadata(
                                    entry.path(),
                                )
                                .ok()?;
                                let kind = if info.is_symlink() {
                                    wire::Listed::Other
                                } else if info.is_dir() {
                                    wire::Listed::Directory
                                } else {
                                    wire::Listed::File(info.len())
                                };
                                Some((
                                    entry
                                        .file_name()
                                        .to_string_lossy()
                                        .into_owned(),
                                    kind,
                                ))
                            })
                            .collect()
                    })
                },
            );
            ok(walked)
        }
        Op::Exec(_) => ok(wire::Ran {
            exit_code: 0,
            stdout: "remote-command-output".into(),
            stderr: String::new(),
        }),
    }
}

async fn send(socket: &mut Socket, mut message: Value) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    message["id"] = id.clone().into();
    socket
        .send(Message::Text(message.to_string().into()))
        .await
        .unwrap();
    id
}

async fn until(
    socket: &mut Socket,
    check: impl Fn(&Value) -> bool,
) -> Value {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let message = socket.next().await.unwrap().unwrap();
            let value = serde_json::from_str::<Value>(
                message.to_text().unwrap(),
            )
            .unwrap();
            assert!(
                !value.to_string().contains(TOKEN),
                "credential reached a client"
            );
            if check(&value) {
                break value;
            }
        }
    })
    .await
    .expect("expected server response")
}

async fn request(socket: &mut Socket, message: Value) -> Value {
    let id = send(socket, message).await;
    until(socket, |value| value["to"] == id).await
}

fn credentials(home: &Scratch, url: &str) {
    fs::write(home.join("credentials.json"),json!({"installationId":"offline-home","connection":{"token":"offline-client-token"},"models":{},"workspace":{"url":url,"token":TOKEN}}).to_string()).unwrap();
}

#[tokio::test]
async fn remote_houses_prompt_gifts_and_backend_move_do_not_open_host_houses()
 {
    let manager = Manager::start();
    let home = Scratch::new("remote-houses");
    credentials(&home, &manager.url);
    let side = Scratch::new("remote-script");
    let script = side.join("faux.json");
    Script::new().write(&script);
    fs::create_dir_all(home.join("houses/home/0")).unwrap();
    fs::write(home.join("houses/home/0/NOTES.md"), "HOST-CANARY")
        .unwrap();
    let mut server =
        Server::start_with_script(&home, &[], Some(&script));
    let (mut socket, state) = server.connect().await;
    assert_eq!(
        state["settings"]["workspace"]["running"],
        manager.url
    );
    let mayor = json!({"hamlet":"home","agent":"mayor"});
    let edited = request(&mut socket,json!({"type":"edit","doc":"notes","agent":mayor,"text":"Remote notes only"})).await;
    assert_eq!(edited["kind"], "notice", "{edited}");
    let papers = request(&mut socket,json!({"type":"read","what":"papers","agent":mayor,"full":true})).await;
    assert_eq!(papers["papers"]["notes"], "Remote notes only");
    let gift = request(&mut socket,json!({"type":"give","to":mayor,"skill":{"starter":"weather"}})).await;
    assert_eq!(gift["kind"], "notice", "{gift}");
    assert!(
        manager.house(0).join("skills/weather/SKILL.md").is_file()
    );
    let upload = request(&mut socket,json!({"type":"give","to":mayor,"files":[{"path":"gift.bin","size":3}]})).await;
    assert_eq!(upload["kind"], "notice", "{upload}");
    let given = request(&mut socket,json!({"type":"give-part","upload":upload["upload"],"path":"gift.bin","offset":0,"data":"/wAB"})).await;
    assert_eq!(given["kind"], "notice", "{given}");
    assert_eq!(
        fs::read(manager.house(0).join("inbox/gift.bin")).unwrap(),
        [255, 0, 1]
    );
    let too_big = request(&mut socket,json!({"type":"give","to":mayor,"files":[{"path":"oversize","size":4194305}]})).await;
    assert_eq!(too_big["kind"], "error", "{too_big}");
    assert!(too_big["text"].as_str().unwrap().contains("4 MiB"));
    let prompt = request(
        &mut socket,
        json!({"type":"read","what":"prompt","agent":mayor}),
    )
    .await
    .to_string();
    assert!(
        prompt.contains("/house")
            && prompt.contains("/library")
            && prompt.contains("Remote notes only")
            && prompt.contains("weather"),
        "{prompt}"
    );
    assert!(!prompt.contains("HOST-CANARY"));
    assert_eq!(
        fs::read_to_string(home.join("houses/home/0/NOTES.md"))
            .unwrap(),
        "HOST-CANARY"
    );
    let refused = request(
        &mut socket,
        json!({"type":"say","to":mayor,"text":"/found west"}),
    )
    .await;
    assert_eq!(refused["kind"], "error", "{refused}");
    fs::create_dir_all(manager.house(0).join("empty/deep")).unwrap();
    fs::write(manager.house(0).join("binary"), [0, 255, 128, 1])
        .unwrap();
    fs::set_permissions(
        manager.house(0).join("binary"),
        fs::Permissions::from_mode(0o751),
    )
    .unwrap();
    symlink("/irrelevant", manager.house(0).join("left-link"))
        .unwrap();
    let target = Manager::start();
    let edited = request(&mut socket,json!({"type":"edit","doc":"settings","set":{"workspace":{"url":target.url,"token":TOKEN}}})).await;
    assert_eq!(edited["kind"], "notice", "{edited}");
    let moved =
        request(&mut socket, json!({"type":"move-houses"})).await;
    assert_eq!(moved["kind"], "notice", "{moved}");
    assert!(moved.to_string().contains("left-link"), "{moved}");
    assert_eq!(
        fs::read(target.house(0).join("binary")).unwrap(),
        [0, 255, 128, 1]
    );
    assert!(target.house(0).join("empty/deep").is_dir());
    assert_eq!(
        fs::metadata(target.house(0).join("binary"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o751
    );
    assert!(!target.house(0).join("left-link").exists());
    let (_, state) = server.connect().await;
    assert_eq!(
        state["settings"]["workspace"]["running"],
        manager.url
    );
    assert_eq!(state["settings"]["workspace"]["url"], target.url);
    assert_eq!(
        state["settings"]["workspace"]["move"]["state"],
        "moved"
    );
    let edited = request(&mut socket,json!({"type":"edit","doc":"settings","set":{"workspace":null}})).await;
    assert_eq!(edited["kind"], "notice", "{edited}");
    let moved =
        request(&mut socket, json!({"type":"move-houses"})).await;
    assert_eq!(moved["kind"], "notice", "{moved}");
    assert_eq!(
        fs::read(home.join("houses/home/0/binary")).unwrap(),
        [0, 255, 128, 1]
    );
    assert_eq!(
        fs::read_to_string(home.join("houses/home/0/NOTES.md"))
            .unwrap(),
        "Remote notes only"
    );
    assert!(manager.house(0).join("binary").is_file());
    server.signal(libc::SIGTERM);
    assert_eq!(server.exit().await, 0);
    drop(server);
    let mut server =
        Server::start_with_script(&home, &[], Some(&script));
    let (mut socket, state) = server.connect().await;
    assert!(state["settings"]["workspace"]["running"].is_null());
    let edited = request(&mut socket,json!({"type":"edit","doc":"settings","set":{"workspace":{"url":target.url,"token":TOKEN}}})).await;
    assert_eq!(edited["kind"], "notice", "{edited}");
    fs::write(home.join("houses/home/0/binary"), [5, 6, 7]).unwrap();
    let moved =
        request(&mut socket, json!({"type":"move-houses"})).await;
    assert_eq!(moved["kind"], "notice", "{moved}");
    assert_eq!(
        fs::read(target.house(0).join("binary")).unwrap(),
        [5, 6, 7]
    );
    assert_eq!(
        fs::read(home.join("houses/home/0/binary")).unwrap(),
        [5, 6, 7]
    );
    server.signal(libc::SIGTERM);
    assert_eq!(server.exit().await, 0);
}

async fn approval(socket: &mut Socket) -> Value {
    let message = until(socket, |value| {
        value["changes"].as_array().is_some_and(|changes| {
            changes.iter().any(|change| {
                change["wait"]["detail"]["kind"] == "approval"
            })
        })
    })
    .await;
    message["changes"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|change| {
            (change["wait"]["detail"]["kind"] == "approval")
                .then(|| change["wait"].clone())
        })
        .unwrap()
}

async fn received(
    manager: &Manager,
    check: impl Fn(&Value) -> bool,
) -> Value {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            for message in manager.received.try_iter() {
                if check(&message) {
                    return message;
                }
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("manager did not receive request")
}

#[tokio::test]
async fn remote_notes_do_not_hold_other_clients_disconnect_or_shutdown()
 {
    for operation in ["read", "write"] {
        let manager = Manager::start();
        let home = Scratch::new("remote-dispatch");
        credentials(&home, &manager.url);
        let side = Scratch::new("remote-dispatch-script");
        let script = side.join("faux.json");
        Script::new().write(&script);
        let mut server = Server::start_with_script(
            &home,
            &[("HAMLET_REFLECT_EVERY", "0")],
            Some(&script),
        );
        let (mut reader, _) = server.connect().await;
        let (mut giver, _) = server.connect().await;
        let upload = request(&mut giver,json!({"type":"give","to":{"hamlet":"home","agent":"mayor"},"files":[{"path":"pending.txt","size":3}]})).await;
        assert_eq!(upload["kind"], "notice", "{upload}");
        assert_eq!(
            fs::read_dir(home.join(".gifts")).unwrap().count(),
            1
        );
        fs::write(
            manager.files.join("hold-request"),
            json!({"kind":operation,"path":"/house/NOTES.md"})
                .to_string(),
        )
        .unwrap();
        let message = if operation == "read" {
            json!({"type":"read","what":"papers","agent":{"hamlet":"home","agent":"mayor"},"full":true})
        } else {
            json!({"type":"edit","doc":"notes","agent":{"hamlet":"home","agent":"mayor"},"text":"# Notes\nheld write"})
        };
        send(&mut reader, message).await;
        received(&manager, |message| {
            message["request"]["operation"]["kind"] == operation
                && message["request"]["operation"]["path"]
                    == "/house/NOTES.md"
        })
        .await;
        let (mut control, _) = tokio::time::timeout(
            Duration::from_secs(3),
            server.connect(),
        )
        .await
        .expect("authentication/state was held behind remote read");
        let stopped = tokio::time::timeout(Duration::from_secs(3), request(&mut control,json!({"type":"stop","agent":{"hamlet":"home","agent":"mayor"}}))).await.expect("stop was held behind remote read");
        assert_eq!(stopped["kind"], "notice", "{stopped}");
        giver.close(None).await.unwrap();
        drop(giver);
        tokio::time::timeout(Duration::from_secs(3), async {
            while fs::read_dir(home.join(".gifts")).unwrap().count()
                != 0
            {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("disconnect did not discard its staged gift");
        server.signal(libc::SIGTERM);
        assert_eq!(server.exit().await, 0);
        if operation == "write" {
            assert_eq!(
                fs::read_to_string(manager.house(0).join("NOTES.md"))
                    .unwrap(),
                "# Notes\nheld write"
            );
            assert!(
                !manager.received.try_iter().any(|message| message
                    ["request"]["operation"]["kind"]
                    == "write"),
                "the uncertain write was replayed"
            );
        }
    }
}

#[tokio::test]
async fn remote_startup_signal_releases_the_home_before_manager_ok() {
    let manager = Manager::start();
    let home = Scratch::new("remote-startup-signal");
    credentials(&home, &manager.url);
    fs::write(
        manager.files.join("hold-request"),
        json!({"kind":"init_house"}).to_string(),
    )
    .unwrap();
    let script = home.join("faux.json");
    Script::new().write(&script);
    let mut server = Server {
        child: Command::new(env!("CARGO_BIN_EXE_hamlet-server"))
            .args(["--port", "0", "--faux"])
            .arg(script)
            .env("HAMLET_HOME", home.as_os_str())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
        url: String::new(),
        token: String::new(),
    };
    received(&manager, |message| {
        message["request"]["operation"]["kind"] == "init_house"
    })
    .await;
    assert!(!home.join("server.json").exists());
    server.signal(libc::SIGTERM);
    assert_eq!(server.exit().await, 0);
    Home::open(&home).unwrap().close().unwrap();
}

#[tokio::test]
async fn remote_tools_approve_house_writes_forward_grants_and_cancel_exact_request()
 {
    let manager = Manager::start();
    let home = Scratch::new("remote-tools");
    let side = Scratch::new("remote-tool-script");
    credentials(&home, &manager.url);
    let script = side.join("faux.json");
    Script::new().responses([
        call("write",json!({"path":"/house/result.txt","content":"approved remotely"})),
        call("read",json!({"path":"/library/weather/SKILL.md"})),
        call("bash",json!({"command":"hold-remote","network":true})),
        words("must not finish the canceled operation"),
    ]).write(&script);
    let mut server = Server::start_with_script(
        &home,
        &[("HAMLET_REFLECT_EVERY", "0")],
        Some(&script),
    );
    let (mut socket, _) = server.connect().await;
    fs::create_dir_all(manager.files.join("home/project")).unwrap();
    let folder = request(&mut socket,json!({"type":"say","to":{"hamlet":"home","agent":"mayor"},"text":"/folder /project"})).await;
    assert_eq!(folder["kind"], "notice", "{folder}");
    send(&mut socket,json!({"type":"say","to":{"hamlet":"home","agent":"mayor"},"text":"Work remotely"})).await;
    let wait = approval(&mut socket).await;
    assert_eq!(wait["detail"]["danger"], "edit", "{wait}");
    assert!(!manager.house(0).join("result.txt").exists());
    send(
        &mut socket,
        json!({"type":"answer","wait":wait["id"],"choice":"allow"}),
    )
    .await;
    let wait = approval(&mut socket).await;
    assert_eq!(wait["detail"]["danger"], "network", "{wait}");
    assert_eq!(
        fs::read_to_string(manager.house(0).join("result.txt"))
            .unwrap(),
        "approved remotely"
    );
    assert!(!home.join("houses/home/0/result.txt").exists());
    send(
        &mut socket,
        json!({"type":"answer","wait":wait["id"],"choice":"allow"}),
    )
    .await;
    let execute = received(&manager, |message| {
        message["request"]["operation"]["argv"][2] == "hold-remote"
    })
    .await;
    let req = &execute["request"];
    assert_eq!(req["workspace"], "home");
    assert_eq!(req["agent"], 0);
    assert_eq!(req["project"], true);
    assert_eq!(req["network"], true);
    assert_eq!(req["read_only"], false);
    assert_eq!(req["operation"]["cwd"], "/project");
    assert!(req["library"]["sha256"].is_string());
    let library = manager.files.join("home/library");
    assert!(library.join("weather/SKILL.md").is_file());
    assert!(!fs::read_dir(&library).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with('.')
    }));
    let stopped = request(&mut socket,json!({"type":"stop","agent":{"hamlet":"home","agent":"mayor"}})).await;
    assert_eq!(stopped["kind"], "notice", "{stopped}");
    let cancel =
        received(&manager, |message| message["kind"] == "cancel")
            .await;
    assert_eq!(cancel["id"], req["id"]);
    server.signal(libc::SIGTERM);
    assert_eq!(server.exit().await, 0);
}

async fn transcript(
    socket: &mut Socket,
    agent: &str,
    text: &str,
) -> Value {
    tokio::time::timeout(Duration::from_secs(20),async {
        loop {
            let shown = request(socket,json!({"type":"open","chat":{"with":{"hamlet":"home","agent":agent}}})).await;
            if shown["items"].to_string().contains(text) {return shown;}
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }).await.expect("expected transcript text")
}

#[tokio::test]
async fn remote_uncertainty_and_host_paths_are_refused_without_fallback_or_retry()
 {
    let manager = Manager::start();
    let home = Scratch::new("remote-refusals");
    let side = Scratch::new("remote-refusals-script");
    let canary = side.join("host-only.txt");
    fs::write(&canary, "PRIVATE-HOST-CANARY").unwrap();
    let script = side.join("faux.json");
    Script::new().responses([
        call("write",json!({"path":"/house/uncertain","content":"one effect"})),
        call("read",json!({"path":canary})),
        words("Refusals finished"),
    ]).write(&script);
    let mut server = Server::start_with_script(
        &home,
        &[
            ("HAMLET_WORKSPACE_URL", &manager.url),
            ("HAMLET_WORKSPACE_TOKEN", TOKEN),
            ("HAMLET_REFLECT_EVERY", "0"),
        ],
        Some(&script),
    );
    let (mut socket, _) = server.connect().await;
    send(&mut socket,json!({"type":"say","to":{"hamlet":"home","agent":"mayor"},"text":"Try both operations"})).await;
    let shown = transcript(&mut socket, "mayor", "Refusals finished")
        .await
        .to_string();
    assert!(
        shown.contains("Inspect") && shown.contains("unknown"),
        "{shown}"
    );
    assert!(!shown.contains("PRIVATE-HOST-CANARY"));
    assert!(shown.contains("manager refused path"), "{shown}");
    let requests = manager.received.try_iter().collect::<Vec<_>>();
    assert_eq!(
        requests
            .iter()
            .filter(|message| message["request"]["operation"]["kind"]
                == "write"
                && message["request"]["operation"]["path"]
                    == "/house/uncertain")
            .count(),
        1
    );
    let refused =
        request(&mut socket, json!({"type":"move-houses"})).await;
    assert_eq!(refused["kind"], "error", "{refused}");
    assert!(
        refused["text"]
            .as_str()
            .unwrap()
            .contains("HAMLET_WORKSPACE_URL")
    );
    assert!(!home.join("houses").exists());
    assert!(
        !fs::read_to_string(home.join("credentials.json"))
            .unwrap()
            .contains(TOKEN)
    );
    assert_eq!(
        fs::read_to_string(manager.house(0).join("uncertain"))
            .unwrap(),
        "one effect"
    );
    server.signal(libc::SIGTERM);
    assert_eq!(server.exit().await, 0);
    let mut server = Server::start_with_script(
        &home,
        &[
            ("HAMLET_WORKSPACE_URL", &manager.url),
            ("HAMLET_WORKSPACE_TOKEN", TOKEN),
            ("HAMLET_REFLECT_EVERY", "0"),
        ],
        Some(&script),
    );
    let (mut socket, _) = server.connect().await;
    let recovered =
        transcript(&mut socket, "mayor", "Refusals finished").await;
    assert!(recovered.to_string().contains("Inspect"));
    server.signal(libc::SIGTERM);
    assert_eq!(server.exit().await, 0);
    assert!(!manager.received.try_iter().any(|message| {
        message["request"]["operation"]["kind"] == "write"
            && message["request"]["operation"]["path"]
                == "/house/uncertain"
    }));
}

#[tokio::test]
async fn remote_hand_tools_are_refused_without_running_on_the_host() {
    let manager = Manager::start();
    let home = Scratch::new("remote-hand");
    let side = Scratch::new("remote-hand-script");
    let canary = side.join("ran");
    let tool = home.join("tools/mark");
    fs::create_dir_all(&tool).unwrap();
    fs::write(
        tool.join("manifest.json"),
        json!({"name":"mark","exec":["touch", canary],"sandbox":{"fs_rw":[side.to_path_buf()]}})
            .to_string(),
    )
    .unwrap();
    let script = side.join("faux.json");
    Script::new()
        .responses([call("mark", json!({})), words("Hand finished")])
        .write(&script);
    let mut server = Server::start_with_script(
        &home,
        &[
            ("HAMLET_WORKSPACE_URL", &manager.url),
            ("HAMLET_WORKSPACE_TOKEN", TOKEN),
            ("HAMLET_REFLECT_EVERY", "0"),
        ],
        Some(&script),
    );
    let (mut socket, _) = server.connect().await;
    send(&mut socket,json!({"type":"say","to":{"hamlet":"home","agent":"mayor"},"text":"Use the hand tool"})).await;
    let shown = transcript(&mut socket, "mayor", "Hand finished")
        .await
        .to_string();
    assert!(shown.contains("remote workspace"), "{shown}");
    assert!(!canary.exists());
    server.signal(libc::SIGTERM);
    assert_eq!(server.exit().await, 0);
}

#[tokio::test]
async fn remote_agents_give_merge_split_and_publish_their_skill() {
    let manager = Manager::start();
    let home = Scratch::new("remote-society");
    credentials(&home, &manager.url);
    let side = Scratch::new("remote-society-script");
    let script = side.join("faux.json");
    Script::new().agent("home:0",[
        call("create_agent",json!({"name":"scout","purpose":"Find","reason":"Explore"})),
        call("create_agent",json!({"name":"smith","purpose":"Make","reason":"Build"})),
        words("Society built"),
    ]).agent("home:1",[
        call("submit_skill",json!({"skill":"map-iron","reason":"Useful maps"})),
        words("Shared maps"),
    ]).errands([words("ACCEPT Useful maps")]).write(&script);
    let mut server = Server::start_with_script(
        &home,
        &[("HAMLET_REFLECT_EVERY", "0")],
        Some(&script),
    );
    let (mut socket, _) = server.connect().await;
    let mayor = json!({"hamlet":"home","agent":"mayor"});
    send(
        &mut socket,
        json!({"type":"say","to":mayor,"text":"Build"}),
    )
    .await;
    transcript(&mut socket, "mayor", "Society built").await;
    assert!(manager.house(1).join("NOTES.md").is_file());
    assert!(manager.house(2).join("NOTES.md").is_file());
    let skill = manager.house(1).join("skills/map-iron");
    fs::create_dir_all(&skill).unwrap();
    fs::write(
        skill.join("SKILL.md"),
        "---\nname: map-iron\ndescription: Maps iron \
         deposits.\n---\nUse this map.\n",
    )
    .unwrap();
    let given = request(&mut socket,json!({"type":"give","to":{"hamlet":"home","agent":"smith"},"skill":{"from":{"hamlet":"home","agent":"scout"},"name":"map-iron"}})).await;
    assert_eq!(given["kind"], "notice", "{given}");
    send(&mut socket,json!({"type":"say","to":{"hamlet":"home","agent":"scout"},"text":"Share"})).await;
    transcript(&mut socket, "scout", "Shared maps").await;
    assert!(home.join("library/map-iron/SKILL.md").is_file());
    assert!(!skill.exists());
    assert!(
        manager.house(2).join("skills/map-iron/SKILL.md").is_file()
    );
    fs::write(manager.house(1).join("clash.txt"), "from scout")
        .unwrap();
    fs::write(manager.house(2).join("clash.txt"), "kept smith")
        .unwrap();
    fs::write(manager.house(1).join("NOTES.md"), "scout notes")
        .unwrap();
    let merged = request(&mut socket,json!({"type":"say","to":mayor,"text":"/merge scout into smith"})).await;
    assert_eq!(merged["kind"], "notice", "{merged}");
    assert_eq!(
        fs::read_to_string(
            manager.house(2).join("merged/scout/clash.txt")
        )
        .unwrap(),
        "from scout"
    );
    assert_eq!(
        fs::read_to_string(manager.house(2).join("clash.txt"))
            .unwrap(),
        "kept smith"
    );
    assert!(
        fs::read_to_string(manager.house(2).join("NOTES.md"))
            .unwrap()
            .contains("scout notes")
    );
    let split = request(
        &mut socket,
        json!({"type":"say","to":mayor,"text":"/restore scout"}),
    )
    .await;
    assert_eq!(split["kind"], "notice", "{split}");
    assert!(
        !manager.house(2).join("merged/scout/clash.txt").exists()
    );
    assert_eq!(
        fs::read_to_string(manager.house(1).join("clash.txt"))
            .unwrap(),
        "from scout"
    );
    let removed = request(
        &mut socket,
        json!({"type":"say","to":mayor,"text":"/unshelve map-iron"}),
    )
    .await;
    assert_eq!(removed["kind"], "notice", "{removed}");
    assert!(!home.join("library/map-iron").exists());
    assert!(!home.join("houses").exists());
    server.signal(libc::SIGTERM);
    assert_eq!(server.exit().await, 0);
}

#[tokio::test]
async fn remote_merge_failure_keeps_listener_record_and_plan_without_replay()
 {
    for uncertain in [false, true] {
        let manager = Manager::start();
        let home = Scratch::new("remote-merge-failure");
        credentials(&home, &manager.url);
        let script = home.join("faux.json");
        Script::new().agent("home:0", [
            call("create_agent",json!({"name":"scout","purpose":"Find","reason":"Explore"})),
            call("create_agent",json!({"name":"smith","purpose":"Make","reason":"Build"})),
            words("Ready to merge"),
        ]).write(&script);
        let mut server = Server::start_with_script(
            &home,
            &[("HAMLET_REFLECT_EVERY", "0")],
            Some(&script),
        );
        let (mut socket, _) = server.connect().await;
        let mayor = json!({"hamlet":"home","agent":"mayor"});
        send(
            &mut socket,
            json!({"type":"say","to":mayor,"text":"Build"}),
        )
        .await;
        transcript(&mut socket, "mayor", "Ready to merge").await;
        fs::write(manager.house(1).join("a.txt"), "first effect")
            .unwrap();
        fs::write(manager.house(1).join("b.txt"), "second effect")
            .unwrap();
        fs::write(
            manager.house(1).join("c.txt"),
            "must not follow failure",
        )
        .unwrap();
        fs::write(manager.files.join("fail-request"), json!({"operation":{"kind":"tree_write","path":"/house/b.txt"},"uncertain":uncertain}).to_string()).unwrap();
        let failed = request(&mut socket,json!({"type":"say","to":mayor,"text":"/merge scout into smith"})).await;
        assert_eq!(failed["kind"], "error", "{failed}");
        assert!(
            failed["text"].as_str().unwrap().contains(if uncertain {
                "unknown"
            } else {
                "refused"
            }),
            "{failed}"
        );
        let (mut control, state) = server.connect().await;
        assert!(state.to_string().contains("smith"));
        let stopped = request(
            &mut control,
            json!({"type":"stop","agent":mayor}),
        )
        .await;
        assert_eq!(stopped["kind"], "notice", "{stopped}");
        fs::remove_file(manager.files.join("fail-request")).unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(
            fs::read_to_string(manager.house(2).join("a.txt"))
                .unwrap(),
            "first effect"
        );
        assert_eq!(
            manager.house(2).join("b.txt").exists(),
            uncertain
        );
        assert!(!manager.house(2).join("c.txt").exists());
        server.signal(libc::SIGTERM);
        assert_eq!(server.exit().await, 0);
        let snapshots = fs::read_dir(home.join(".society-files"))
            .unwrap()
            .count();
        assert!(snapshots > 0);
        let mut stored = Home::open(&home).unwrap();
        let record = stored.store().record("home").unwrap().unwrap();
        assert!(record.named("scout").is_none());
        assert!(
            !record
                .archive
                .last()
                .unwrap()
                .merge
                .as_ref()
                .unwrap()
                .applied
        );
        stored.close().unwrap();
        let mut server = Server::start_with_script(
            &home,
            &[("HAMLET_REFLECT_EVERY", "0")],
            Some(&script),
        );
        let (_, state) = server.connect().await;
        assert!(state.to_string().contains("smith"));
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(
            manager.house(2).join("b.txt").exists(),
            uncertain
        );
        assert!(
            !manager.house(2).join("c.txt").exists(),
            "restart silently continued an unconfirmed merge"
        );
        assert_eq!(
            fs::read_dir(home.join(".society-files"))
                .unwrap()
                .count(),
            snapshots
        );
        server.signal(libc::SIGTERM);
        assert_eq!(server.exit().await, 0);
        assert_eq!(
            manager
                .received
                .try_iter()
                .filter(|message| message["request"]["operation"]
                    ["kind"]
                    == "tree_write"
                    && message["request"]["operation"]["path"]
                        == "/house/b.txt")
                .count(),
            1
        );
    }
}

#[tokio::test]
async fn remote_domain_prompt_and_gate_waits_release_service_dispatch()
 {
    for scenario in [
        "create",
        "gift",
        "library",
        "library-clean",
        "vote",
        "merge",
        "split",
        "prompt",
        "gate",
    ] {
        let manager = Manager::start();
        let home = Scratch::new(&format!("remote-held-{scenario}"));
        credentials(&home, &manager.url);
        let script = home.join("faux.json");
        let next = match scenario {
            "create" => call(
                "create_agent",
                json!({"name":"third","purpose":"Help","reason":"Needed"}),
            ),
            "gate" => call(
                "write",
                json!({"path":"/house/review.txt","content":"must not write"}),
            ),
            _ => words("Must not finish a held prompt"),
        };
        let offering = if scenario == "vote" {
            call(
                "call_vote",
                json!({"kind":"found_hamlet","hamlet":"mill","mayor":"miller","purpose":"Grind flour","reason":"New trade"}),
            )
        } else {
            call(
                "submit_skill",
                json!({"skill":"maps","reason":"Useful maps"}),
            )
        };
        let errands = if scenario == "vote" {
            vec![
                call(
                    "cast_ballot",
                    json!({"choice":"yes","reason":"New trade","evidence":""}),
                ),
                call(
                    "cast_ballot",
                    json!({"choice":"yes","reason":"New trade","evidence":""}),
                ),
                call(
                    "cast_ballot",
                    json!({"choice":"yes","reason":"New trade","evidence":""}),
                ),
            ]
        } else {
            vec![words("ACCEPT Useful maps")]
        };
        let mut initial = vec![
            call(
                "create_agent",
                json!({"name":"scout","purpose":"Find","reason":"Explore"}),
            ),
            call(
                "create_agent",
                json!({"name":"smith","purpose":"Make","reason":"Build"}),
            ),
        ];
        if scenario == "vote" {
            initial.push(call("create_agent",json!({"name":"poet","purpose":"Write","reason":"Third voter"})));
        }
        initial.extend([words("Built"), next]);
        Script::new()
            .agent("home:0", initial)
            .agent("home:1", [offering, words("Shared")])
            .errands(errands)
            .write(&script);
        let mut server = Server::start_with_script(
            &home,
            &[("HAMLET_REFLECT_EVERY", "0")],
            Some(&script),
        );
        let (mut socket, _) = server.connect().await;
        let mayor = json!({"hamlet":"home","agent":"mayor"});
        send(
            &mut socket,
            json!({"type":"say","to":mayor,"text":"Build"}),
        )
        .await;
        transcript(&mut socket, "mayor", "Built").await;
        fs::create_dir_all(manager.house(1).join("skills/maps"))
            .unwrap();
        fs::write(
            manager.house(1).join("skills/maps/SKILL.md"),
            "---\nname: maps\ndescription: Read maps.\n---\nRead \
             the map.\n",
        )
        .unwrap();
        fs::write(manager.house(1).join("a.txt"), "source").unwrap();
        fs::write(manager.house(0).join("review.txt"), "unchanged")
            .unwrap();
        if scenario == "split" {
            let result = request(&mut socket,json!({"type":"say","to":mayor,"text":"/merge scout into smith"})).await;
            assert_eq!(result["kind"], "notice", "{result}");
        }
        let (hold, message) = match scenario {
            "create" => (
                json!({"kind":"init_house"}),
                json!({"type":"say","to":mayor,"text":"Create another"}),
            ),
            "gift" => (
                json!({"kind":"tree_list","path":"/house/skills/maps"}),
                json!({"type":"give","to":{"hamlet":"home","agent":"smith"},"skill":{"from":{"hamlet":"home","agent":"scout"},"name":"maps"}}),
            ),
            "library" => (
                json!({"kind":"tree_list","path":"/house/skills/maps"}),
                json!({"type":"say","to":{"hamlet":"home","agent":"scout"},"text":"Share"}),
            ),
            "library-clean" => (
                json!({"kind":"remove","path":"/house/skills/maps"}),
                json!({"type":"say","to":{"hamlet":"home","agent":"scout"},"text":"Share"}),
            ),
            "vote" => (
                json!({"kind":"init_house"}),
                json!({"type":"say","to":{"hamlet":"home","agent":"scout"},"text":"Found a mill"}),
            ),
            "merge" => (
                json!({"kind":"tree_list","path":"/house"}),
                json!({"type":"say","to":mayor,"text":"/merge scout into smith"}),
            ),
            "split" => (
                json!({"kind":"remove","path":"/house/a.txt"}),
                json!({"type":"say","to":mayor,"text":"/restore scout"}),
            ),
            "prompt" => (
                json!({"kind":"read","path":"/house/NOTES.md"}),
                json!({"type":"say","to":mayor,"text":"Think"}),
            ),
            "gate" => (
                json!({"kind":"read","path":"/house/review.txt"}),
                json!({"type":"say","to":mayor,"text":"Write"}),
            ),
            _ => unreachable!(),
        };
        manager.received.try_iter().for_each(drop);
        fs::write(
            manager.files.join("hold-request"),
            hold.to_string(),
        )
        .unwrap();
        eprintln!("holding {scenario}: {hold}");
        send(&mut socket, message).await;
        received(&manager, |message| {
            hold.as_object().unwrap().iter().all(|(key, value)| {
                message["request"]["operation"][key] == *value
            })
        })
        .await;
        let (mut control, _) = tokio::time::timeout(
            Duration::from_secs(3),
            server.connect(),
        )
        .await
        .unwrap_or_else(|_| {
            panic!("{scenario} blocks authentication/state")
        });
        let stopped = tokio::time::timeout(Duration::from_secs(3), request(&mut control, json!({"type":"stop","agent":{"hamlet":"home","agent":if scenario == "library" {"scout"} else {"mayor"}}}))).await.unwrap_or_else(|_| panic!("{scenario} blocks stop"));
        assert_eq!(
            stopped["kind"], "notice",
            "{scenario}: {stopped}"
        );
        socket.close(None).await.unwrap();
        drop(socket);
        server.signal(libc::SIGTERM);
        assert_eq!(server.exit().await, 0, "{scenario}");
        assert_eq!(
            fs::read_to_string(manager.house(0).join("review.txt"))
                .unwrap(),
            "unchanged"
        );
        assert!(!home.join("houses").exists());
    }
}

#[tokio::test]
async fn remote_merge_completion_preserves_fresh_record_and_rejects_replaced_identity()
 {
    for recreate in [false, true] {
        let manager = Manager::start();
        let home = Scratch::new("remote-merge-race");
        credentials(&home, &manager.url);
        let script = home.join("faux.json");
        Script::new().agent("home:0", [
            call("create_agent",json!({"name":"scout","purpose":"Find","reason":"Explore"})),
            call("create_agent",json!({"name":"smith","purpose":"Make","reason":"Build"})),
            words("Built"),
            call("create_agent",json!({"name":if recreate {"scout"} else {"third"},"purpose":"New identity","reason":"Concurrent edit"})),
            words("Concurrent edit finished"),
        ]).write(&script);
        let mut server = Server::start_with_script(
            &home,
            &[("HAMLET_REFLECT_EVERY", "0")],
            Some(&script),
        );
        let (mut socket, _) = server.connect().await;
        let mayor = json!({"hamlet":"home","agent":"mayor"});
        send(
            &mut socket,
            json!({"type":"say","to":mayor,"text":"Build"}),
        )
        .await;
        transcript(&mut socket, "mayor", "Built").await;
        fs::write(manager.house(1).join("a.txt"), "original scout")
            .unwrap();
        let hold = if recreate {
            json!({"kind":"tree_list","path":"/house"})
        } else {
            json!({"kind":"tree_write","path":"/house/a.txt"})
        };
        manager.received.try_iter().for_each(drop);
        fs::write(
            manager.files.join("hold-request"),
            hold.to_string(),
        )
        .unwrap();
        let merging = send(&mut socket,json!({"type":"say","to":mayor,"text":"/merge scout into smith"})).await;
        received(&manager, |message| {
            hold.as_object().unwrap().iter().all(|(key, value)| {
                message["request"]["operation"][key] == *value
            })
        })
        .await;
        let (mut control, _) = server.connect().await;
        if recreate {
            let removed = request(&mut control,json!({"type":"say","to":mayor,"text":"/remove scout"})).await;
            assert_eq!(removed["kind"], "notice", "{removed}");
        }
        send(&mut control,json!({"type":"say","to":mayor,"text":"Create new identity"})).await;
        transcript(&mut control, "mayor", "Concurrent edit finished")
            .await;
        fs::remove_file(manager.files.join("hold-request")).unwrap();
        let merged =
            until(&mut socket, |value| value["to"] == merging).await;
        if recreate {
            assert_eq!(merged["kind"], "error", "{merged}");
            assert!(
                merged["text"]
                    .as_str()
                    .unwrap()
                    .contains("No replacement agent was merged"),
                "{merged}"
            );
            assert!(!manager.house(2).join("a.txt").exists());
        } else {
            assert_eq!(merged["kind"], "notice", "{merged}");
            assert_eq!(
                fs::read_to_string(manager.house(2).join("a.txt"))
                    .unwrap(),
                "original scout"
            );
        }
        server.signal(libc::SIGTERM);
        assert_eq!(server.exit().await, 0);
        let mut stored = Home::open(&home).unwrap();
        let record = stored.store().record("home").unwrap().unwrap();
        assert_eq!(
            record
                .named(if recreate { "scout" } else { "third" })
                .unwrap()
                .number,
            3
        );
        assert_eq!(
            record
                .archive
                .iter()
                .filter(|entry| entry.merge.is_some())
                .count(),
            usize::from(!recreate)
        );
        stored.close().unwrap();
    }
}

#[tokio::test]
async fn remote_completed_merge_write_is_not_replayed_after_lost_reply_and_host_crash()
 {
    let manager = Manager::start();
    let home = Scratch::new("remote-merge-crash");
    credentials(&home, &manager.url);
    let script = home.join("faux.json");
    Script::new().agent("home:0", [
        call("create_agent",json!({"name":"scout","purpose":"Find","reason":"Explore"})),
        call("create_agent",json!({"name":"smith","purpose":"Make","reason":"Build"})),
        words("Built"),
    ]).write(&script);
    let mut server = Server::start_with_script(
        &home,
        &[("HAMLET_REFLECT_EVERY", "0")],
        Some(&script),
    );
    let (mut socket, _) = server.connect().await;
    let mayor = json!({"hamlet":"home","agent":"mayor"});
    send(
        &mut socket,
        json!({"type":"say","to":mayor,"text":"Build"}),
    )
    .await;
    transcript(&mut socket, "mayor", "Built").await;
    fs::write(manager.house(1).join("NOTES.md"), "scout's knowledge")
        .unwrap();
    fs::write(
        manager.files.join("hold-request"),
        json!({"kind":"write","path":"/house/NOTES.md"}).to_string(),
    )
    .unwrap();
    manager.received.try_iter().for_each(drop);
    send(&mut socket,json!({"type":"say","to":mayor,"text":"/merge scout into smith"})).await;
    received(&manager, |message| {
        message["request"]["operation"]["kind"] == "write"
            && message["request"]["operation"]["path"]
                == "/house/NOTES.md"
    })
    .await;
    tokio::time::timeout(Duration::from_secs(3), async {
        while !fs::read_to_string(manager.house(2).join("NOTES.md"))
            .unwrap()
            .contains("scout's knowledge")
        {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let (mut control, _) = server.connect().await;
    let stopped =
        request(&mut control, json!({"type":"stop","agent":mayor}))
            .await;
    assert_eq!(stopped["kind"], "notice", "{stopped}");
    server.signal(libc::SIGKILL);
    assert!(!server.child.wait().unwrap().success());
    drop(server);
    let snapshots =
        fs::read_dir(home.join(".society-files")).unwrap().count();
    assert!(snapshots > 0);
    fs::remove_file(manager.files.join("hold-request")).unwrap();
    let mut server = Server::start_with_script(
        &home,
        &[("HAMLET_REFLECT_EVERY", "0")],
        Some(&script),
    );
    let (mut socket, _) = server.connect().await;
    let split = request(
        &mut socket,
        json!({"type":"say","to":mayor,"text":"/restore scout"}),
    )
    .await;
    assert_eq!(split["kind"], "error", "{split}");
    assert!(
        split["text"].as_str().unwrap().contains("unknown effects"),
        "{split}"
    );
    tokio::time::sleep(Duration::from_millis(200)).await;
    server.signal(libc::SIGTERM);
    assert_eq!(server.exit().await, 0);
    assert!(!manager.received.try_iter().any(|message| message
        ["request"]["operation"]["kind"]
        == "write"
        && message["request"]["operation"]["path"]
            == "/house/NOTES.md"));
    assert_eq!(
        fs::read_dir(home.join(".society-files")).unwrap().count(),
        snapshots
    );
    let mut stored = Home::open(&home).unwrap();
    let record = stored.store().record("home").unwrap().unwrap();
    let entry = record.archive.last().unwrap();
    assert!(!entry.merge.as_ref().unwrap().applied);
    assert!(entry.back.is_none());
    stored.close().unwrap();
}

#[tokio::test]
async fn remote_preview_rechecks_mode_after_wait_and_same_socket_keeps_order()
 {
    let manager = Manager::start();
    let home = Scratch::new("remote-gate-order");
    credentials(&home, &manager.url);
    let script = home.join("faux.json");
    Script::new().responses([
        call("write",json!({"path":"/house/review.txt","content":"must be denied"})),
        words("Reviewed"),
    ]).write(&script);
    let mut server = Server::start_with_script(
        &home,
        &[("HAMLET_REFLECT_EVERY", "0")],
        Some(&script),
    );
    let (mut socket, _) = server.connect().await;
    let mayor = json!({"hamlet":"home","agent":"mayor"});
    fs::create_dir_all(manager.files.join("home/project")).unwrap();
    let folder = request(
        &mut socket,
        json!({"type":"say","to":mayor,"text":"/folder /project"}),
    )
    .await;
    assert_eq!(folder["kind"], "notice", "{folder}");
    let mode = request(
        &mut socket,
        json!({"type":"say","to":mayor,"text":"/mode auto"}),
    )
    .await;
    assert_eq!(mode["kind"], "notice", "{mode}");
    fs::write(manager.house(0).join("review.txt"), "old text")
        .unwrap();
    fs::write(
        manager.files.join("hold-request"),
        json!({"kind":"read","path":"/house/review.txt"}).to_string(),
    )
    .unwrap();
    send(
        &mut socket,
        json!({"type":"say","to":mayor,"text":"Write"}),
    )
    .await;
    received(&manager, |message| {
        message["request"]["operation"]["kind"] == "read"
            && message["request"]["operation"]["path"]
                == "/house/review.txt"
    })
    .await;
    let (mut control, _) = server.connect().await;
    let mode = request(
        &mut control,
        json!({"type":"say","to":mayor,"text":"/mode manual"}),
    )
    .await;
    assert_eq!(mode["kind"], "notice", "{mode}");
    fs::remove_file(manager.files.join("hold-request")).unwrap();
    let wait = approval(&mut socket).await;
    assert_eq!(wait["detail"]["danger"], "edit", "{wait}");
    request(
        &mut control,
        json!({"type":"answer","wait":wait["id"],"choice":"deny"}),
    )
    .await;
    transcript(&mut socket, "mayor", "Reviewed").await;
    assert_eq!(
        fs::read_to_string(manager.house(0).join("review.txt"))
            .unwrap(),
        "old text"
    );
    manager.received.try_iter().for_each(drop);
    fs::write(
        manager.files.join("hold-request"),
        json!({"kind":"write","path":"/house/NOTES.md"}).to_string(),
    )
    .unwrap();
    let first = send(&mut socket,json!({"type":"edit","doc":"notes","agent":mayor,"text":"# Notes\nordered edit"})).await;
    received(&manager, |message| {
        message["request"]["operation"]["kind"] == "write"
    })
    .await;
    let second = send(&mut socket,json!({"type":"read","what":"papers","agent":mayor,"full":true})).await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(
        !manager.received.try_iter().any(|message| message
            ["request"]["operation"]["kind"]
            == "read"
            && message["request"]["operation"]["path"]
                == "/house/NOTES.md"),
        "same-socket read overtook its edit"
    );
    fs::remove_file(manager.files.join("hold-request")).unwrap();
    let written = until(&mut socket, |value| {
        value["to"] == first || value["to"] == second
    })
    .await;
    assert_eq!(written["to"], first);
    assert_eq!(written["kind"], "notice", "{written}");
    let read =
        until(&mut socket, |value| value["to"] == second).await;
    assert!(read.to_string().contains("ordered edit"), "{read}");
    server.signal(libc::SIGTERM);
    assert_eq!(server.exit().await, 0);
}

#[tokio::test]
async fn remote_model_merge_finishes_before_a_queued_chief_restore() {
    let manager = Manager::start();
    let home = Scratch::new("remote-model-merge");
    credentials(&home, &manager.url);
    let script = home.join("faux.json");
    Script::new().agent("home:0", [
        call("create_agent",json!({"name":"scout","purpose":"Find","reason":"Explore"})),
        call("create_agent",json!({"name":"smith","purpose":"Make","reason":"Build"})),
        words("Built"),
        call("merge_agents",json!({"absorb":"scout","keep":"smith","purpose":"Combined work","reason":"Combine"})),
        words("Model merge finished"),
    ]).errands([words("# Notes\nCombined work")]).write(&script);
    let mut server = Server::start_with_script(
        &home,
        &[("HAMLET_REFLECT_EVERY", "0")],
        Some(&script),
    );
    let (mut socket, _) = server.connect().await;
    let mayor = json!({"hamlet":"home","agent":"mayor"});
    send(
        &mut socket,
        json!({"type":"say","to":mayor,"text":"Build"}),
    )
    .await;
    transcript(&mut socket, "mayor", "Built").await;
    fs::write(manager.house(1).join("a.txt"), "source").unwrap();
    fs::write(
        manager.files.join("hold-request"),
        json!({"kind":"tree_list","path":"/house"}).to_string(),
    )
    .unwrap();
    manager.received.try_iter().for_each(drop);
    send(
        &mut socket,
        json!({"type":"say","to":mayor,"text":"Merge"}),
    )
    .await;
    received(&manager, |message| {
        message["request"]["operation"]["kind"] == "tree_list"
            && message["request"]["operation"]["path"] == "/house"
    })
    .await;
    let (mut control, _) = server.connect().await;
    let restoring = send(
        &mut control,
        json!({"type":"say","to":mayor,"text":"/restore scout"}),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(
        !manager
            .received
            .try_iter()
            .any(|message| message["request"]["operation"]["kind"]
                == "remove")
    );
    fs::remove_file(manager.files.join("hold-request")).unwrap();
    let restored =
        until(&mut control, |value| value["to"] == restoring).await;
    assert_eq!(restored["kind"], "notice", "{restored}");
    let shown =
        transcript(&mut socket, "mayor", "Model merge finished")
            .await;
    assert!(
        shown["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["tool"]["name"] == "merge_agents"
                && item["tool"]["exit"] == 0),
        "{shown}"
    );
    assert!(!manager.house(2).join("a.txt").exists());
    assert_eq!(
        fs::read_to_string(manager.house(1).join("a.txt")).unwrap(),
        "source"
    );
    server.signal(libc::SIGTERM);
    assert_eq!(server.exit().await, 0);
}
