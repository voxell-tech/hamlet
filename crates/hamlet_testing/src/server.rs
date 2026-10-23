//! A server process on a scratch home, as `hamlet_acceptance`'s
//! README says it is started, and a raw connection to it.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use futures::{SinkExt, StreamExt};
use hamlet_protocol::client::{RequestId, V};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::Scratch;
use crate::script::Script;

/// Selects the command that starts a server.
const SERVER_VAR: &str = "HAMLET_ACCEPTANCE_SERVER";

/// The longest a server may take to publish its address.
const START: Duration = Duration::from_secs(30);

/// The longest a server may take to end.
const END: Duration = Duration::from_secs(10);

/// The longest a connection waits for its first state.
const HELLO: Duration = Duration::from_secs(3);

/// A WebSocket connection to a server.
pub type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// The models a server answers with.
pub enum Models<'a> {
    /// The faux script in this file, given with `--faux`.
    Faux(&'a Path),
    /// Its real model providers.
    Providers,
}

/// A home and the folder beside it for scripts, kept across the
/// servers started on it.
pub struct Home {
    home: Scratch,
    side: Scratch,
    started: u32,
}

impl Home {
    pub fn new() -> Self {
        Self {
            home: Scratch::new("home"),
            side: Scratch::new("side"),
            started: 0,
        }
    }

    /// The home's folder, given to the server as `HAMLET_HOME`.
    pub fn path(&self) -> &Path {
        &self.home
    }

    /// A folder of the test's own, outside the home.
    pub fn side(&self) -> &Path {
        &self.side
    }

    /// The path of `relative` in the home.
    pub fn file(&self, relative: &str) -> PathBuf {
        self.path().join(relative)
    }

    /// Starts a server on this home answering with `script`, with
    /// `env` added to its environment.
    pub async fn serve(
        &mut self,
        script: &Script,
        env: &[(&str, &str)],
    ) -> Server {
        self.launch(script, env)
            .await
            .unwrap_or_else(|failure| panic!("{failure}"))
    }

    /// Starts a server that is expected to fail, and gives what it
    /// printed.
    pub async fn try_serve(
        &mut self,
        script: &Script,
        env: &[(&str, &str)],
    ) -> String {
        match self.launch(script, env).await {
            Ok(_) => panic!("the server started"),
            Err(failure) => failure,
        }
    }

    /// Starts a server on this home with its real model providers,
    /// without `--faux`. `env` points them at a loopback endpoint.
    pub async fn serve_providers(
        &mut self,
        env: &[(&str, &str)],
    ) -> Server {
        Server::start(
            &server_command(),
            self.path(),
            Models::Providers,
            env,
        )
        .await
        .unwrap_or_else(|failure| panic!("{failure}"))
    }

    async fn launch(
        &mut self,
        script: &Script,
        env: &[(&str, &str)],
    ) -> Result<Server, String> {
        self.started += 1;
        let file =
            self.side().join(format!("script-{}.json", self.started));
        script.write(&file);
        Server::start(
            &server_command(),
            self.path(),
            Models::Faux(&file),
            env,
        )
        .await
    }
}

impl Default for Home {
    fn default() -> Self {
        Self::new()
    }
}

/// A running server, killed on drop.
pub struct Server {
    child: Child,
    log: PathBuf,
    /// The protocol's WebSocket URL, from `server.json`.
    pub url: String,
    /// The connection token, from `credentials.json`.
    pub token: String,
    // The folder of `log`, removed with the server.
    _logs: Scratch,
}

#[derive(Deserialize)]
struct Published {
    url: String,
    pid: u32,
}

#[derive(Deserialize)]
struct Credentials {
    connection: Connection,
}

#[derive(Deserialize)]
struct Connection {
    token: String,
}

impl Server {
    /// Runs `command`, a program and its first arguments, on `home`
    /// with `--port 0`, and waits until it publishes its address.
    /// Its output goes to a log of its own. A server that cannot
    /// start is an error; so is one that ends or publishes nothing,
    /// with the log's end.
    pub async fn start(
        command: &[String],
        home: &Path,
        models: Models<'_>,
        env: &[(&str, &str)],
    ) -> Result<Self, String> {
        let (program, args) =
            command.split_first().expect("a command to run");
        let logs = Scratch::new("server-log");
        let log = logs.join("server.log");
        let output = std::fs::File::create(&log).unwrap();
        let mut command = Command::new(program);
        crate::strip_overrides(&mut command);
        command.args(args).arg("--port").arg("0");
        if let Models::Faux(script) = models {
            command.arg("--faux").arg(script);
        }
        let started = command
            .env("HAMLET_HOME", home)
            .env("HAMLET_USER", "chief")
            .envs(env.iter().copied())
            .stdin(Stdio::null())
            .stdout(output.try_clone().unwrap())
            .stderr(output)
            .spawn();
        let words = || {
            std::iter::once(program)
                .chain(args)
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(" ")
        };
        let child = started.map_err(|error| {
            format!("cannot start the server `{}`: {error}", words())
        })?;
        let mut server = Self {
            child,
            log,
            url: String::new(),
            token: String::new(),
            _logs: logs,
        };
        let pid = server.child.id();
        let published = home.join("server.json");
        let began = Instant::now();
        server.url = loop {
            if let Some(status) = server.child.try_wait().unwrap() {
                return Err(format!(
                    "the server `{}` ended with {status} before it \
                     published its address:\n{}",
                    words(),
                    tail(&server.log)
                ));
            }
            let found = std::fs::read_to_string(&published)
                .ok()
                .and_then(|text| {
                    serde_json::from_str::<Published>(&text).ok()
                });
            if let Some(found) =
                found.filter(|found| found.pid == pid)
            {
                break found.url;
            }
            assert!(
                began.elapsed() < START,
                "the server `{}` published no address in \
                 {START:?}:\n{}",
                words(),
                tail(&server.log)
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        let credentials =
            std::fs::read_to_string(home.join("credentials.json"))
                .unwrap_or_else(|error| {
                    panic!(
                        "the server wrote no credentials.json: \
                         {error}"
                    )
                });
        server.token =
            serde_json::from_str::<Credentials>(&credentials)
                .expect("credentials.json holds connection.token")
                .connection
                .token;
        Ok(server)
    }

    /// Connects, says hello with the token and gives the first
    /// state.
    pub async fn connect(&self) -> (Socket, Value) {
        let (mut socket, _) =
            tokio_tungstenite::connect_async(&self.url)
                .await
                .unwrap();
        socket
            .send(Message::Text(
                json!({"type": "hello", "v": V, "token": self.token})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
        let state = tokio::time::timeout(HELLO, socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let state =
            serde_json::from_str::<Value>(state.to_text().unwrap())
                .unwrap();
        assert_eq!(state["type"], "state");
        (socket, state)
    }

    /// Kills the server at once, as a crash would, and gives how it
    /// ended.
    pub fn kill(&mut self) -> ExitStatus {
        // An error says it had ended already.
        let _ = self.child.kill();
        self.child.wait().unwrap()
    }

    /// Asks the server to stop with SIGTERM, and gives its exit code.
    pub async fn stop(&mut self) -> Option<i32> {
        self.terminate();
        self.ended().await
    }

    /// Sends SIGTERM without waiting.
    pub fn terminate(&mut self) {
        self.signal(libc::SIGTERM);
    }

    /// Sends SIGINT, as Ctrl-C does, without waiting.
    pub fn interrupt(&mut self) {
        self.signal(libc::SIGINT);
    }

    /// Waits for the server to end, and gives its exit code.
    pub async fn ended(&mut self) -> Option<i32> {
        tokio::time::timeout(END, async {
            loop {
                if let Some(status) = self.child.try_wait().unwrap() {
                    break status.code();
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("the server ends within {END:?}"))
    }

    fn signal(&mut self, signal: i32) {
        // SAFETY: kill sends a signal to our own child's PID, which
        // has not been reaped.
        let sent =
            unsafe { libc::kill(self.child.id() as i32, signal) };
        assert_eq!(sent, 0, "the server is running");
    }

    /// Whether the process is still running.
    pub fn running(&mut self) -> bool {
        self.child.try_wait().unwrap().is_none()
    }

    /// What the server printed so far.
    pub fn log(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Sends `requests` on `socket` and gives the answer to each, in
/// the order they came.
pub async fn answers(
    socket: &mut Socket,
    requests: &[&Value],
) -> Vec<Value> {
    for request in requests {
        socket
            .send(Message::Text(request.to_string().into()))
            .await
            .unwrap();
    }
    let mut answers = Vec::new();
    tokio::time::timeout(Duration::from_secs(3), async {
        while answers.len() < requests.len() {
            let frame = socket.next().await.unwrap().unwrap();
            let message = serde_json::from_str::<Value>(
                frame.to_text().unwrap(),
            )
            .unwrap();
            if requests.iter().any(|r| r["id"] == message["to"]) {
                answers.push(message);
            }
        }
    })
    .await
    .unwrap();
    answers
}

/// Sends `message` on `socket` with a new id, and gives the id.
pub async fn send(socket: &mut Socket, mut message: Value) -> String {
    let id = RequestId::new().to_string();
    message["id"] = id.clone().into();
    socket
        .send(Message::Text(message.to_string().into()))
        .await
        .unwrap();
    id
}

/// Reads `socket` until a message passes `check`, and gives it. A
/// check that asserts sees every message before it.
pub async fn until(
    socket: &mut Socket,
    check: impl Fn(&Value) -> bool,
) -> Value {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let frame = socket.next().await.unwrap().unwrap();
            let value = serde_json::from_str::<Value>(
                frame.to_text().unwrap(),
            )
            .unwrap();
            if check(&value) {
                break value;
            }
        }
    })
    .await
    .expect("the server did not send the expected message")
}

/// The command that starts a server, before `--port 0` and
/// `--faux <file>`: [`SERVER_VAR`], else the `hamlet` CLI's
/// `server`.
fn server_command() -> Vec<String> {
    if let Some(command) = std::env::var(SERVER_VAR)
        .ok()
        .filter(|v| !v.trim().is_empty())
    {
        let mut words = command
            .split_whitespace()
            .map(String::from)
            .collect::<Vec<_>>();
        let program = Path::new(&words[0]);
        if program.is_relative() && program.components().count() > 1 {
            words[0] = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join(program)
                .display()
                .to_string();
        }
        return words;
    }
    let cli = cli();
    assert!(
        cli.is_file(),
        "{} is missing: build it with `cargo build -p hamlet_cli`, \
         set HAMLET_CLI to a built CLI, or set {SERVER_VAR} to the \
         command that starts a server",
        cli.display()
    );
    vec![cli.display().to_string(), "server".into()]
}

/// The `hamlet` CLI: `HAMLET_CLI`, else the workspace's debug build.
pub fn cli() -> PathBuf {
    std::env::var_os("HAMLET_CLI")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/debug/hamlet")
        })
}

/// The last lines of a log, for a failure's message.
fn tail(log: &Path) -> String {
    let text = std::fs::read_to_string(log).unwrap_or_default();
    let lines = text.lines().collect::<Vec<_>>();
    lines[lines.len().saturating_sub(40)..].join("\n")
}
