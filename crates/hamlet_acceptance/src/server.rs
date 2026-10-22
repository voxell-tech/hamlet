//! Starting a server process on a scratch home, as the README's
//! contract says.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use serde::Deserialize;
use tokio::process::{Child, Command};

use crate::script::Script;

/// Selects the command that starts a server.
pub const SERVER_VAR: &str = "HAMLET_ACCEPTANCE_SERVER";

/// The longest a server may take to publish its address.
const START: Duration = Duration::from_secs(30);

/// A scratch folder, removed on drop.
pub struct Scratch(PathBuf);

impl Scratch {
    /// A new, empty folder under the system's temp folder.
    pub fn new(tag: &str) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "hamlet-acceptance-{tag}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // The real path: on macOS the temp folder is a symlink.
        Self(dir.canonicalize().unwrap())
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A home and the folder beside it for scripts and logs, kept across
/// the servers started on it.
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
        self.home.path()
    }

    /// A folder of the test's own, outside the home.
    pub fn side(&self) -> &Path {
        self.side.path()
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
        self.started += 1;
        let log =
            self.side().join(format!("server-{}.log", self.started));
        Server::start(self.path(), None, &log, env)
            .await
            .unwrap_or_else(|failure| panic!("{failure}"))
    }

    async fn launch(
        &mut self,
        script: &Script,
        env: &[(&str, &str)],
    ) -> Result<Server, String> {
        self.started += 1;
        let script_file =
            self.side().join(format!("script-{}.json", self.started));
        script.write(&script_file);
        let log =
            self.side().join(format!("server-{}.log", self.started));
        Server::start(self.path(), Some(&script_file), &log, env)
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
    async fn start(
        home: &Path,
        script: Option<&Path>,
        log: &Path,
        env: &[(&str, &str)],
    ) -> Result<Self, String> {
        let words = server_command();
        let (program, args) = words.split_first().unwrap();
        let output = std::fs::File::create(log).unwrap();
        let mut command = Command::new(program);
        hamlet_testing::strip_overrides(command.as_std_mut());
        command.args(args).arg("--port").arg("0");
        if let Some(script) = script {
            command.arg("--faux").arg(script);
        }
        let mut child = command
            .env("HAMLET_HOME", home)
            .env("HAMLET_USER", "chief")
            .envs(env.iter().copied())
            .stdin(Stdio::null())
            .stdout(output.try_clone().unwrap())
            .stderr(output)
            .kill_on_drop(true)
            .spawn()
            .unwrap_or_else(|error| {
                panic!(
                    "cannot start the server `{}`: {error}. Set \
                     {SERVER_VAR} to the command that starts one.",
                    words.join(" ")
                )
            });
        let pid = child.id().unwrap();
        let published = home.join("server.json");
        let began = Instant::now();
        let url = loop {
            if let Some(status) = child.try_wait().unwrap() {
                return Err(format!(
                    "the server `{}` ended with {status} before it \
                     published its address:\n{}",
                    words.join(" "),
                    tail(log)
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
                words.join(" "),
                tail(log)
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        };
        let credentials =
            std::fs::read_to_string(home.join("credentials.json"))
                .unwrap_or_else(|error| {
                    panic!(
                        "the server wrote no credentials.json: \
                         {error}"
                    )
                });
        let token = serde_json::from_str::<Credentials>(&credentials)
            .expect("credentials.json holds connection.token")
            .connection
            .token;
        Ok(Self {
            child,
            log: log.into(),
            url,
            token,
        })
    }

    /// Kills the server at once, as a crash would.
    pub async fn kill(mut self) {
        self.child.kill().await.unwrap();
    }

    /// Asks the server to stop with SIGTERM, and gives its exit code.
    pub async fn stop(mut self) -> Option<i32> {
        self.signal(libc::SIGTERM);
        self.ended().await
    }

    /// Sends SIGINT, as Ctrl-C does, without waiting.
    pub fn interrupt(&mut self) {
        self.signal(libc::SIGINT);
    }

    /// Waits up to 10s for the server to end, and gives its exit
    /// code.
    pub async fn ended(mut self) -> Option<i32> {
        let status = tokio::time::timeout(
            Duration::from_secs(10),
            self.child.wait(),
        )
        .await
        .expect("the server ends within 10s")
        .unwrap();
        status.code()
    }

    fn signal(&mut self, signal: i32) {
        let pid = self.child.id().expect("the server is running");
        // SAFETY: `pid` is our own child, which has not been reaped.
        unsafe { libc::kill(pid as i32, signal) };
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

/// The command that starts a server, before `--port 0` and
/// `--faux <file>`.
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
