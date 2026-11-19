//! One MCP server kept running: its process under the sandbox, the
//! MCP client that speaks with it over the process's standard input
//! and output, and a start again with a growing delay when it ends.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Duration;

use bevy_ecs::message::Message;
use bevy_log::{debug, warn};
use hamlet_os::process::Tree;
use hamlet_sandbox::scrub::{Scrubber, scrub_text};
use hamlet_sandbox::{Env, ExecError, Piped, Policy, Sandbox};
use rmcp::model::{
    ClientCapabilities, ClientConfig, Implementation,
    SubscriptionFilter,
};
use rmcp::service::{
    ClientInitializeError, NotificationContext, RunningService,
    ServiceError, Subscription,
};
use rmcp::{ClientHandler, RoleClient, ServiceExt};
use serde_json::Value;
use tokio::io::{
    AsyncBufReadExt, AsyncRead, AsyncReadExt, BufReader,
};
use tokio::process::ChildStderr;
use tokio::sync::mpsc;
use tokio::task::{AbortHandle, JoinHandle};
use tokio::time::Instant;

use crate::app::input::InputSender;

/// The way to a running server's client, which calls may share.
pub(crate) type Peer = rmcp::Peer<RoleClient>;

/// A tool a server lists, as it lists it.
pub(crate) type Tool = rmcp::model::Tool;

/// How long a server has to finish starting, and to list its tools.
const HANDSHAKE: Duration = Duration::from_secs(30);

/// The first wait before a server that ended starts again, doubled
/// each time up to `LONGEST_DELAY`.
const FIRST_DELAY: Duration = Duration::from_secs(2);
const LONGEST_DELAY: Duration = Duration::from_secs(300);

/// A session that lasted this long starts the delays over.
const STABLE: Duration = Duration::from_secs(60);

/// How long a server whose connection closed has to end, so that its
/// exit tells why; and its error output to close once it ended.
const EXITING: Duration = Duration::from_secs(1);

/// How many times a session opens again a subscription to its
/// server's changes that ended.
const LISTENS: usize = 3;

/// The most of one line of a server's error output that is kept.
const ERROR_LINE: usize = 1024;

/// How many lines of a server's error output the log is given when it
/// ends.
const ERROR_LINES: usize = 20;

/// What a session says of its server.
pub(crate) enum Status {
    /// The server is running and lists these tools.
    Up {
        tools: Vec<Tool>,
        peer: Peer,
        /// The hash of the policy it runs under.
        policy: String,
        /// What of the policy this machine cannot hold.
        degraded: Value,
    },
    /// It ended, and starts again after `again`.
    Down { reason: String, again: Duration },
}

/// A session's report, which the server takes in as an input.
#[derive(Message)]
pub(crate) struct Report {
    pub server: String,
    /// The start of the list it belongs to; a later one replaces it.
    pub generation: u64,
    pub status: Status,
}

/// What a server starts from.
pub(super) struct Launch {
    pub name: String,
    pub generation: u64,
    pub argv: Vec<String>,
    pub policy: Policy,
    pub sandbox: Sandbox,
    /// Its own folder, which it works in.
    pub home: PathBuf,
    /// What must not reach the log from what it says.
    pub secrets: Vec<String>,
}

/// Why a session ended.
#[derive(Debug, thiserror::Error)]
enum Stop {
    #[error("its folder could not be made: {0}")]
    Folder(std::io::Error),
    /// The sandbox's or the program's own words.
    #[error("{0}")]
    Unstarted(String),
    /// Boxed, as the client's error is larger than the rest
    /// together.
    #[error("it did not start: {0}")]
    Refused(Box<ClientInitializeError>),
    #[error("it did not list its tools: {0}")]
    Unlisted(ServiceError),
    #[error("it did not answer within {} seconds", HANDSHAKE.as_secs())]
    Slow,
    /// Its process's exit code or signal.
    #[error("it ended with {0}")]
    Ended(String),
    #[error("its output ended")]
    Closed,
    #[error("the server is stopping")]
    Stopping,
}

/// Starts keeping `launch`'s server running, until the handle aborts
/// it.
pub(super) fn start(
    launch: Launch,
    inputs: InputSender,
) -> AbortHandle {
    tokio::spawn(keep(launch, inputs)).abort_handle()
}

/// Runs the server's sessions one after another, each after a longer
/// delay while they keep ending soon.
async fn keep(launch: Launch, inputs: InputSender) {
    let mut delay = FIRST_DELAY;
    loop {
        let began = Instant::now();
        let (stop, said) = session(&launch, &inputs).await;
        if began.elapsed() >= STABLE {
            delay = FIRST_DELAY;
        }
        let reason = scrub_text(&launch.secrets, &stop.to_string());
        let said = said.into_iter().collect::<Vec<_>>().join("\n");
        warn!(
            "MCP server {} stopped: {reason}; it starts again in \
             {}{}",
            launch.name,
            super::seconds(delay),
            if said.is_empty() {
                String::new()
            } else {
                format!(". It last said:\n{said}")
            }
        );
        let report = Report {
            server: launch.name.clone(),
            generation: launch.generation,
            status: Status::Down {
                reason,
                again: delay,
            },
        };
        if matches!(stop, Stop::Stopping)
            || inputs.send(report).is_err()
        {
            return;
        }
        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(LONGEST_DELAY);
    }
}

/// One session: the server started and served until it ends. Gives
/// why it ended, and the last lines of its error output.
async fn session(
    launch: &Launch,
    inputs: &InputSender,
) -> (Stop, VecDeque<String>) {
    if let Err(error) = std::fs::create_dir_all(&launch.home) {
        return (Stop::Folder(error), VecDeque::new());
    }
    let env = match Env::private(
        &hamlet_os::fs::path::text(&launch.home),
        launch.policy.clone(),
        launch.sandbox,
    ) {
        Ok(env) => env,
        Err(error) => {
            return (
                Stop::Unstarted(error.to_string()),
                VecDeque::new(),
            );
        }
    };
    let Piped {
        mut tree,
        stdin,
        stdout,
        stderr,
    } = match env.spawn_piped(&launch.argv) {
        Ok(piped) => piped,
        Err(ExecError::Spawn(words) | ExecError::Spill(words)) => {
            return (Stop::Unstarted(words), VecDeque::new());
        }
        Err(ExecError::Timeout { .. }) => {
            return (
                Stop::Unstarted("it did not start".into()),
                VecDeque::new(),
            );
        }
    };
    let reader = Reader::start(stderr, launch);
    let ran = Ran {
        tree: &mut tree,
        launch,
        inputs,
        env: &env,
    };
    let stop = ran.serve(stdout, stdin).await;
    // Its error output closes once every process of it has ended.
    drop(tree);
    (stop, reader.said().await)
}

/// A started server's process, and what it is reported to.
struct Ran<'a> {
    tree: &'a mut Tree,
    launch: &'a Launch,
    inputs: &'a InputSender,
    env: &'a Env,
}

impl Ran<'_> {
    /// Speaks with the server over `stdout` and `stdin`, and keeps
    /// what it lists offered until it ends. Gives why it ended.
    async fn serve(
        self,
        stdout: tokio::process::ChildStdout,
        stdin: tokio::process::ChildStdin,
    ) -> Stop {
        let (changed, mut changes) = mpsc::channel(1);
        let started = tokio::time::timeout(
            HANDSHAKE,
            Client { changed }.serve((stdout, stdin)),
        );
        let client = tokio::select! {
            ended = self.tree.ended() => return Stop::Ended(word(ended)),
            started = started => match started {
                Ok(Ok(client)) => client,
                Ok(Err(error)) => return self.exit_or(Stop::Refused(Box::new(error))).await,
                Err(_) => return Stop::Slow,
            },
        };
        let peer = client.peer().clone();
        let mut subscription = listen(&peer).await;
        // A subscription that ends is opened again, a few times at
        // most, so a server cannot keep the session busy with it.
        let mut listens =
            usize::from(subscription.is_some()) * LISTENS;
        if let Err(stop) = self.offer(&peer).await {
            return stop;
        }
        let closed = closed(client);
        tokio::pin!(closed);
        loop {
            tokio::select! {
                ended = self.tree.ended() => return Stop::Ended(word(ended)),
                () = &mut closed => return self.exit_or(Stop::Closed).await,
                Some(()) = changes.recv() => {}
                Some(()) = next(&mut subscription) => {}
            }
            if subscription.is_none() && listens > 0 {
                listens -= 1;
                subscription = listen(&peer).await;
            }
            if let Err(stop) = self.offer(&peer).await {
                return stop;
            }
        }
    }

    /// Reports the tools the server lists.
    async fn offer(&self, peer: &Peer) -> Result<(), Stop> {
        let tools = tools_of(peer).await?;
        let report = Report {
            server: self.launch.name.clone(),
            generation: self.launch.generation,
            status: Status::Up {
                tools,
                peer: peer.clone(),
                policy: self.env.policy_hash().to_owned(),
                degraded: self.env.degraded(),
            },
        };
        self.inputs.send(report).map_err(|_| Stop::Stopping)
    }

    /// How the process ended, once it ends soon; else `stop`.
    async fn exit_or(self, stop: Stop) -> Stop {
        match tokio::time::timeout(EXITING, self.tree.ended()).await {
            Ok(ended) => Stop::Ended(word(ended)),
            Err(_) => stop,
        }
    }
}

/// The tools the server lists, over every page; none when it offers
/// no tools.
async fn tools_of(peer: &Peer) -> Result<Vec<Tool>, Stop> {
    let offers = peer
        .peer_info()
        .is_some_and(|info| info.capabilities.tools.is_some());
    if !offers {
        return Ok(Vec::new());
    }
    tokio::time::timeout(HANDSHAKE, peer.list_all_tools())
        .await
        .map_err(|_| Stop::Slow)?
        .map_err(Stop::Unlisted)
}

/// The subscription to the changes of the server's tools, on a
/// version of the protocol that sends them only to one; none when the
/// server sends them to every client, or never.
async fn listen(peer: &Peer) -> Option<Subscription> {
    let info = peer.peer_info()?;
    let changes = info
        .capabilities
        .tools
        .as_ref()
        .and_then(|tools| tools.list_changed)
        == Some(true);
    if !changes || info.protocol_version.has_initialize() {
        return None;
    }
    let filter =
        SubscriptionFilter::builder().tools_list_changed().build();
    tokio::time::timeout(HANDSHAKE, peer.listen(filter))
        .await
        .ok()?
        .inspect_err(|error| {
            debug!(
                "an MCP server refused to tell its changes: {error}"
            );
        })
        .ok()
}

/// The next change `subscription` tells, or that it ended, which may
/// have lost one; never when there is none.
async fn next(subscription: &mut Option<Subscription>) -> Option<()> {
    if let Some(listening) = subscription {
        if !matches!(listening.next().await, Ok(Some(_))) {
            *subscription = None;
        }
        return Some(());
    }
    std::future::pending().await
}

/// Ends once the client's connection closes; the client closes when
/// this is dropped.
async fn closed(client: RunningService<RoleClient, Client>) {
    let _ = client.waiting().await;
}

/// The words of how a server's process ended.
fn word(ended: std::io::Result<std::process::ExitStatus>) -> String {
    match ended {
        Ok(status) => match status.code() {
            Some(code) => format!("exit code {code}"),
            None => hamlet_sandbox::status_word(&status),
        },
        Err(error) => error.to_string(),
    }
}

/// The client's side of the protocol: it offers a server nothing to
/// ask of it (ADR 0017), and hears when its tools change.
struct Client {
    changed: mpsc::Sender<()>,
}

impl ClientHandler for Client {
    fn get_info(&self) -> ClientConfig {
        ClientConfig::new(
            ClientCapabilities::default(),
            Implementation::new("hamlet", env!("CARGO_PKG_VERSION")),
        )
    }

    async fn on_tool_list_changed(
        &self,
        _context: NotificationContext<RoleClient>,
    ) {
        // One waiting change covers any more that come meanwhile.
        let _ = self.changed.try_send(());
    }
}

/// The reading of a server's error output, which stops when this
/// drops.
struct Reader {
    task: JoinHandle<VecDeque<String>>,
    abort: AbortHandle,
}

impl Reader {
    /// Logs each line of `stderr` without the server's secrets, and
    /// keeps the last of them.
    fn start(stderr: ChildStderr, launch: &Launch) -> Self {
        let task = tokio::spawn(read(
            stderr,
            launch.name.clone(),
            launch.secrets.clone(),
        ));
        Self {
            abort: task.abort_handle(),
            task,
        }
    }

    /// The last lines read, once the output closes soon; else none.
    async fn said(mut self) -> VecDeque<String> {
        tokio::time::timeout(EXITING, &mut self.task)
            .await
            .ok()
            .and_then(Result::ok)
            .unwrap_or_default()
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        self.abort.abort();
    }
}

/// The last lines of `from`, the error output of the server `name`,
/// each logged without `secrets` as it comes.
async fn read(
    from: impl AsyncRead + Unpin,
    name: String,
    secrets: Vec<String>,
) -> VecDeque<String> {
    let mut reader = BufReader::new(from);
    // One scrubber for the whole output takes out a secret that a cut
    // line splits.
    let mut scrubber = Scrubber::new(&secrets);
    let mut lines = VecDeque::new();
    loop {
        let mut line = Vec::new();
        // A longer line is cut, and the rest of it read as lines of
        // its own.
        let read = (&mut reader)
            .take(u64::try_from(ERROR_LINE).unwrap_or(u64::MAX))
            .read_until(b'\n', &mut line)
            .await;
        let text = if matches!(read, Ok(1..)) {
            scrubber.push(&String::from_utf8_lossy(&line))
        } else {
            scrubber.flush()
        };
        let text = text.trim_end();
        if !text.is_empty() {
            debug!("MCP server {name}: {text}");
            if lines.len() == ERROR_LINES {
                lines.pop_front();
            }
            lines.push_back(text.to_owned());
        }
        if !matches!(read, Ok(1..)) {
            return lines;
        }
    }
}
