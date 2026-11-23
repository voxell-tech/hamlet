//! The web IDE on a hamlet's folders: one OpenVSCode Server for each
//! hamlet, started when a window first asks for it, under the
//! sandbox with the folders it works on alone to write. The folders
//! it worked on before show too, read-only. It listens on this
//! machine only and takes a token, which the address it gives holds.

use std::collections::BTreeMap;
use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::Result;
use bevy_app::App;
use bevy_ecs::message::Messages;
use bevy_ecs::resource::Resource;
use bevy_ecs::system::ResMut;
use hamlet_os::fs::path::text;
use hamlet_os::process::{Command, Limits, Tree};
use hamlet_protocol::client::{self as protocol, ServerMessage};
use hamlet_sandbox::{Env, Policy, Sandbox, host_env};
use hamlet_store::ids::HamletId;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::net::TcpStream;
use tokio::task::AbortHandle;
use tokio::time::{Instant, sleep};
use url::Url;

use crate::app::input::AddHandler;
use crate::app::tasks::BackgroundResult;
use crate::clients::handlers::FromClient;
use crate::core::Core;
use crate::society::{authority, hamlets};

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<WebIdes>()
        .add_handler::<FromClient<protocol::WebIde>, _>(asked);
}

/// Answers each window that asked for a web IDE: at once with one
/// that runs, else once the one started listens.
fn asked(
    mut received: ResMut<Messages<FromClient<protocol::WebIde>>>,
    mut core: Core,
    mut ides: ResMut<WebIdes>,
) -> bevy_ecs::error::Result {
    received
        .drain()
        .try_for_each(|FromClient { client, request }| {
            let to = request.id.clone();
            let hamlet = HamletId(request.hamlet.clone());
            match address(&mut core, &mut ides, &hamlet)? {
                Address::Now(answer) => {
                    core.answer(client, reply(to, answer))?;
                }
                Address::Starting {
                    port,
                    address,
                    alive,
                } => {
                    core.later.spawn(async move {
                        let answer = match listening(port).await {
                            true => Ok(address),
                            false => {
                                // The next ask starts another.
                                alive.store(false, Ordering::Relaxed);
                                Err("The web IDE did not start \
                                     listening."
                                    .to_owned())
                            }
                        };
                        BackgroundResult::Reply {
                            client,
                            message: reply(to, answer),
                        }
                    });
                }
            }
            Ok(())
        })
}

/// The reply to the ask `to`: the web IDE's address, or why there is
/// none.
fn reply(
    to: protocol::RequestId,
    answer: Result<String, String>,
) -> ServerMessage {
    let (kind, text) = match answer {
        Ok(address) => (protocol::ReplyKind::Address, address),
        Err(words) => (protocol::ReplyKind::Error, words),
    };
    ServerMessage::Reply(protocol::Reply {
        file: None,
        files: Vec::new(),
        image: None,
        images: Vec::new(),
        kind,
        text,
        to,
        upload: None,
    })
}

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

/// The web IDE servers that run, by the hamlet each is of.
#[derive(Resource, Default)]
pub(crate) struct WebIdes {
    running: BTreeMap<HamletId, Running>,
}

/// The folders a web IDE shows.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Folders {
    /// Those the hamlet works on, the project first: the server may
    /// write them.
    working: Vec<PathBuf>,
    /// Those it worked on before, which the server only reads.
    past: Vec<PathBuf>,
}

impl Folders {
    /// The project, where the server runs.
    fn project(&self) -> &Path {
        &self.working[0]
    }

    /// Whether it is the project alone, opened as a folder rather
    /// than a workspace of several.
    fn is_one(&self) -> bool {
        self.working.len() == 1 && self.past.is_empty()
    }

    /// Each folder, those worked on first, with whether it is past
    /// and the name its root shows: its own, or its whole path where
    /// another has the same. The page finds a file's folder by it.
    fn roots(&self) -> Vec<(&Path, bool, String)> {
        let all = self
            .working
            .iter()
            .map(|folder| (folder.as_path(), false))
            .chain(
                self.past
                    .iter()
                    .map(|folder| (folder.as_path(), true)),
            )
            .collect::<Vec<_>>();
        let name = |folder: &Path| {
            folder.file_name().map_or_else(
                || text(folder),
                |name| text(Path::new(name)),
            )
        };
        all.iter()
            .map(|&(folder, past)| {
                let own = name(folder);
                let shared = all
                    .iter()
                    .filter(|(other, _)| name(other) == own)
                    .count()
                    > 1;
                (
                    folder,
                    past,
                    if shared { text(folder) } else { own },
                )
            })
            .collect()
    }
}

struct Running {
    port: u16,
    /// The folders it was started on; it starts anew when they
    /// change, since the sandbox's are fixed.
    folders: Folders,
    /// Where it opens.
    address: String,
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
    /// The address of `hamlet`'s server, if one runs on `folders`.
    /// One on other folders is ended.
    fn alive(
        &mut self,
        hamlet: &HamletId,
        folders: &Folders,
    ) -> Option<String> {
        let running = self.running.get(hamlet)?;
        if running.alive.load(Ordering::Relaxed)
            && running.folders == *folders
        {
            return Some(running.address.clone());
        }
        self.running.remove(hamlet);
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

/// A web IDE's address, or why there is none, now; or the one that
/// just started, once it listens on `port`.
enum Address {
    Now(Result<String, String>),
    Starting {
        port: u16,
        address: String,
        alive: Arc<AtomicBool>,
    },
}

/// The address of a web IDE on `hamlet`'s folders, started if it
/// does not run; or why there is none.
fn address(
    core: &mut Core<'_, '_>,
    ides: &mut WebIdes,
    hamlet: &HamletId,
) -> Result<Address> {
    let folders = match folders_of(core, hamlet)? {
        Ok(folders) => folders,
        Err(words) => return Ok(Address::Now(Err(words))),
    };
    if let Some(address) = ides.alive(hamlet, &folders) {
        return Ok(Address::Now(Ok(address)));
    }
    let Some(program) = std::env::var_os(PROGRAM) else {
        return Ok(Address::Now(Err(format!(
            "No web IDE is set up: {PROGRAM} names no \
             openvscode-server."
        ))));
    };
    let started = match start(
        &core.tools.sandbox.0,
        Path::new(&program),
        folders,
        core.realm.layout().root(),
    ) {
        Ok(started) => started,
        Err(words) => return Ok(Address::Now(Err(words))),
    };
    let starting = Address::Starting {
        port: started.port,
        address: started.address.clone(),
        alive: started.alive.clone(),
    };
    ides.running.insert(hamlet.clone(), started);
    Ok(starting)
}

/// The folders of `hamlet`, or why this chief cannot have a web IDE
/// on them.
fn folders_of(
    core: &mut Core<'_, '_>,
    hamlet: &HamletId,
) -> Result<Result<Folders, String>> {
    if core.remote.workspace.is_some() {
        return Ok(Err("The web IDE runs on this machine, and the \
                       agents work in a remote workspace."
            .into()));
    }
    let chief = core.realm.chief();
    let db = core.realm.store().connection();
    let living = hamlets::find(db, hamlet)?
        .is_some_and(|found| found.is_living());
    if !living {
        return Ok(Err(format!("There is no hamlet `{hamlet}`.")));
    }
    if !authority::is_chief_of(db, chief, hamlet)? {
        return Ok(Err(format!(
            "The hamlet `{hamlet}` is not yours."
        )));
    }
    let working: Vec<PathBuf> = hamlets::working(db, hamlet)?
        .into_iter()
        .map(PathBuf::from)
        .collect();
    if working.is_empty() {
        return Ok(Err(format!(
            "The hamlet `{hamlet}` works on no folder."
        )));
    }
    let past = hamlets::folders(db, hamlet)?
        .into_iter()
        .map(PathBuf::from)
        .filter(|folder| !working.contains(folder))
        .collect();
    Ok(Ok(Folders { working, past }))
}

/// Starts `program` on `folders`, confined to write those worked on
/// alone and to read all but `realm`.
fn start(
    sandbox: &Sandbox,
    program: &Path,
    folders: Folders,
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
        rw: folders.working.clone(),
        open: true,
        hidden: vec![realm.to_owned()],
        env: host_env(),
        net: true,
        ..Policy::default()
    };
    let env = Env::new(&text(folders.project()), policy, *sandbox)
        .map_err(|error| {
            format!("The sandbox could not be set up: {error}")
        })?;
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
    let opened = if folders.is_one() {
        Opened::Folder(folders.project().to_owned())
    } else {
        let file = tmp.join("hamlet.code-workspace");
        std::fs::write(&file, workspace(&folders)).map_err(
            |error| {
                format!(
                    "The web IDE's workspace could not be written: \
                     {error}"
                )
            },
        )?;
        Opened::Workspace(file, folders.clone())
    };
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
        address: open_address(port, &token, &opened),
        folders,
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
    // Begun anew, so an extension no longer given is gone.
    if extensions.exists() {
        std::fs::remove_dir_all(extensions)?;
    }
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
        "--disable-workspace-trust".into(),
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

/// What a web IDE opens: the project alone, or a workspace file of
/// all the folders.
enum Opened {
    Folder(PathBuf),
    Workspace(PathBuf, Folders),
}

/// The workspace file of `folders`: each a root, those worked on
/// before after the rest and read-only.
fn workspace(folders: &Folders) -> String {
    let roots = folders
        .roots()
        .into_iter()
        .map(|(folder, _, name)| {
            serde_json::json!({ "path": text(folder), "name": name })
        })
        .collect::<Vec<_>>();
    let readonly = folders
        .past
        .iter()
        .map(|folder| (format!("{}/**", text(folder)), true.into()))
        .collect::<serde_json::Map<_, _>>();
    serde_json::json!({
        "folders": roots,
        "settings": { "files.readonlyInclude": readonly },
    })
    .to_string()
}

/// The address of the server on `port` with `token`, opened on
/// `opened`. A workspace's address also lists its folders and which
/// are past, for the page's own parts.
fn open_address(port: u16, token: &str, opened: &Opened) -> String {
    let mut address =
        Url::parse(&format!("http://127.0.0.1:{port}/"))
            .expect("a loopback address");
    let mut query = address.query_pairs_mut();
    query.append_pair("tkn", token);
    match opened {
        Opened::Folder(folder) => {
            query.append_pair("folder", &text(folder));
        }
        Opened::Workspace(file, folders) => {
            let roots = folders
                .roots()
                .into_iter()
                .map(|(folder, past, name)| {
                    serde_json::json!({
                        "path": text(folder),
                        "past": past,
                        "name": name,
                    })
                })
                .collect::<Vec<_>>();
            query.append_pair("workspace", &text(file)).append_pair(
                "roots",
                &serde_json::Value::from(roots).to_string(),
            );
        }
    }
    drop(query);
    address.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_address_holds_the_token_and_the_folder_encoded() {
        let address = open_address(
            3000,
            "abc",
            &Opened::Folder(PathBuf::from("/tmp/my folder")),
        );
        assert_eq!(
            address,
            "http://127.0.0.1:3000/?tkn=abc&folder=%2Ftmp%2Fmy+folder"
        );
    }

    fn folders() -> Folders {
        Folders {
            working: vec!["/w/app".into(), "/w/docs".into()],
            past: vec!["/w/old".into()],
        }
    }

    #[test]
    fn a_workspace_lists_the_past_folders_last_and_read_only() {
        let file: serde_json::Value =
            serde_json::from_str(&workspace(&folders())).unwrap();
        assert_eq!(
            file["folders"],
            serde_json::json!([
                { "path": "/w/app", "name": "app" },
                { "path": "/w/docs", "name": "docs" },
                { "path": "/w/old", "name": "old" },
            ])
        );
        assert_eq!(
            file["settings"]["files.readonlyInclude"],
            serde_json::json!({ "/w/old/**": true })
        );
    }

    #[test]
    fn a_workspace_s_address_names_its_file_and_roots_and_no_folder()
    {
        let address = Url::parse(&open_address(
            3000,
            "abc",
            &Opened::Workspace(
                "/t/h.code-workspace".into(),
                folders(),
            ),
        ))
        .unwrap();
        let query = address
            .query_pairs()
            .into_owned()
            .collect::<BTreeMap<_, _>>();
        assert!(!query.contains_key("folder"));
        assert_eq!(query["workspace"], "/t/h.code-workspace");
        let roots: serde_json::Value =
            serde_json::from_str(&query["roots"]).unwrap();
        assert_eq!(
            roots[0],
            serde_json::json!({ "path": "/w/app", "past": false, "name": "app" })
        );
        assert_eq!(
            roots[2],
            serde_json::json!({ "path": "/w/old", "past": true, "name": "old" })
        );
    }

    #[test]
    fn roots_of_the_same_name_are_named_by_their_whole_path() {
        let folders = Folders {
            working: vec!["/a/src".into(), "/b/src".into()],
            past: vec!["/c/docs".into()],
        };
        let names = folders
            .roots()
            .into_iter()
            .map(|(_, _, name)| name)
            .collect::<Vec<_>>();
        assert_eq!(names, ["/a/src", "/b/src", "docs"]);
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
