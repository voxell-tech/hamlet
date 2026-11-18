//! The Node host as the Rust host's child process.
//!
//! A child has ended here once it has exited and its tree is killed.

use std::collections::{BTreeMap, HashMap};
use std::io::{self, PipeReader, PipeWriter};
use std::path::Path;
use std::process::ExitStatus;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::{Context, Result};
use hamlet_actor_core::ActorFactory;
use hamlet_os::process::{self, Command, Pipes, Tree};
use parking_lot::Mutex;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::Instant;

pub use crate::link::LinkState;
use crate::link::{self, AtBreak, Broken, Frame, Port, Role, Writer};
use crate::local::Inbound;
use crate::route::Routes;
use crate::{Host, HostOptions};

/// The child's variable naming the realm folder it opens.
pub(crate) const HOME: &str = "HAMLET_HOME";

/// The child's whole environment: each variable, and where its value
/// comes from. An inherited variable the Rust host does not have, or
/// whose value is not UTF-8, is left out.
const ENVIRONMENT: &[(&str, Value)] = &[
    (HOME, Value::Home),
    ("PATH", Value::Inherited),
    ("HAMLET_REFLECT_EVERY", Value::Inherited),
    ("HAMLET_CLAUDE_EXECUTABLE", Value::Inherited),
    ("HAMLET_ACTOR_", Value::InheritedPrefix),
    ("CLAUDE_CONFIG_DIR", Value::Inherited),
    ("HOME", Value::Inherited),
    ("TMPDIR", Value::Inherited),
    ("HTTP_PROXY", Value::Inherited),
    ("HTTPS_PROXY", Value::Inherited),
    ("NO_PROXY", Value::Inherited),
    #[cfg(not(windows))]
    ("http_proxy", Value::Inherited),
    #[cfg(not(windows))]
    ("https_proxy", Value::Inherited),
    #[cfg(not(windows))]
    ("no_proxy", Value::Inherited),
    ("NODE_EXTRA_CA_CERTS", Value::Inherited),
    ("SSL_CERT_FILE", Value::Inherited),
    ("SSL_CERT_DIR", Value::Inherited),
    #[cfg(windows)]
    ("USERPROFILE", Value::Inherited),
    #[cfg(windows)]
    ("APPDATA", Value::Inherited),
    #[cfg(windows)]
    ("LOCALAPPDATA", Value::Inherited),
    #[cfg(windows)]
    ("SystemRoot", Value::Inherited),
    #[cfg(windows)]
    ("TEMP", Value::Inherited),
    #[cfg(windows)]
    ("TMP", Value::Inherited),
    #[cfg(feature = "faults")]
    ("HAMLET_FAULT_DIR", Value::Inherited),
];

/// How long after a child has ended the next one starts, at first
/// and after a child that stayed ready for [`STEADY`].
const RESTART: Duration = Duration::from_secs(1);

/// The longest pause between two children.
const RESTART_CAP: Duration = Duration::from_secs(60);

/// How long a child stays ready for the pause after it to go back to
/// [`RESTART`].
const STEADY: Duration = Duration::from_secs(60);

/// How long a child has to say hello, from its start.
const HELLO_DEADLINE: Duration = Duration::from_secs(30);

/// How long a welcomed child has to say it is ready.
const READY_DEADLINE: Duration = Duration::from_secs(120);

/// How long a child has to exit after the goodbye, or once its link
/// is closed.
const EXIT_DEADLINE: Duration = Duration::from_secs(15);

/// The link's ends in the Node host, from [`take_link`] until
/// [`link::serve`] takes them.
static ENDS: Mutex<Option<Ends>> = Mutex::new(None);

/// Where the value of a variable of [`ENVIRONMENT`] comes from.
enum Value {
    Home,
    /// The Rust host's own environment.
    Inherited,
    /// Each variable of the Rust host's own environment whose name
    /// starts with the given one.
    InheritedPrefix,
}

/// The Node child of a host.
#[derive(Clone, Debug)]
pub struct ChildOptions {
    /// The program and its arguments.
    pub argv: Vec<String>,
    /// The kinds the child runs. Its hello must state exactly these,
    /// and the host sends their actors' calls and creates to it.
    pub kinds: Vec<String>,
}

/// Opens the folder `dir` as [`Host::open_with`] does, with the
/// kinds of `child` running in a Node child that the returned
/// [`Supervisor`] keeps running, in `dir`. A call to, or a create of,
/// an actor of those kinds goes to the child. The child's actors
/// call this host's.
///
/// Must be called in the tokio runtime the host's actors run on,
/// which runs the child's calls. Fails outside one, for a kind both
/// hosts would hold, and for a folder whose path is not UTF-8. At the
/// end, shut the supervisor down first, then the host.
pub fn open(
    dir: &Path,
    factories: HashMap<String, ActorFactory>,
    options: HostOptions,
    child: ChildOptions,
) -> Result<(Host, Supervisor)> {
    let home = dir
        .to_str()
        .context("the realm folder's path is not UTF-8")?
        .to_owned();
    let ours = link::kinds(&factories);
    let routes = options.routes.clone();
    let port = Port::new(Role::Rust, child.kinds);
    let host = Host::open_linked(
        dir,
        factories,
        options,
        Some(port.clone()),
    )?;
    let supervised = Supervised {
        argv: child.argv,
        home,
        ours,
        routes,
        host: Inbound::of(&host)?,
        port,
    };
    let supervisor = Supervisor::start(supervised)
        .context("start the Node host's supervisor")?;
    Ok((host, supervisor))
}

struct Supervised {
    argv: Vec<String>,
    home: String,
    ours: Vec<String>,
    /// The route table the welcome gives the child.
    routes: Routes,
    port: Port,
    host: Inbound,
}

/// Keeps one Node child running, from a thread of its own. A child
/// that exits, breaks the link, misses a deadline of the handshake or
/// is refused is killed with its tree and followed by a new one after
/// a pause, for as long as the supervisor runs.
///
/// Dropped, it stops: its thread sees the drop and shuts the running
/// child down as [`Supervisor::shutdown`] does, and the drop does not
/// wait for it.
pub struct Supervisor {
    /// Dropped, it stops the thread.
    stop: oneshot::Sender<()>,
    state: watch::Receiver<LinkState>,
    thread: JoinHandle<Option<ExitStatus>>,
}

impl Supervisor {
    /// Starts the supervisor's thread, which starts the first child
    /// and logs to the subscriber that is the default here.
    fn start(supervised: Supervised) -> io::Result<Self> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let (stop, stopped) = oneshot::channel();
        let state = supervised.port.state();
        let logs = tracing::dispatcher::get_default(Clone::clone);
        let thread = thread::Builder::new()
            .name("node-link".to_owned())
            .spawn(move || {
                let _logs = tracing::dispatcher::set_default(&logs);
                runtime.block_on(supervise(&supervised, stopped))
            })?;
        Ok(Self {
            stop,
            state,
            thread,
        })
    }

    /// The link's state, which changes only when it moves.
    pub fn state(&self) -> watch::Receiver<LinkState> {
        self.state.clone()
    }

    /// Starts no more children, fails the calls out to the running
    /// one with `link.lost`, says goodbye to it, gives it 15 seconds
    /// to exit, then kills its tree. Returns how that child ended, or
    /// `None` when no child was running, or when its end could not be
    /// waited for.
    pub async fn shutdown(self) -> Option<ExitStatus> {
        let Self { stop, thread, .. } = self;
        drop(stop);
        match tokio::task::spawn_blocking(move || thread.join()).await
        {
            Ok(Ok(status)) => status,
            Ok(Err(panic)) => std::panic::resume_unwind(panic),
            // Only the end of the caller's runtime cancels the join.
            Err(_) => None,
        }
    }
}

/// How a child's session ended.
enum End {
    /// The child is to go, and a new one start.
    Lost,
    /// The link is closed, by the child or by a refusal of its hello,
    /// and the child is to say why on its errors and exit.
    Closed,
    /// The supervisor shuts down.
    Shutdown,
}

/// A child the supervisor started.
struct Running {
    tree: Tree,
    out: Writer,
    /// What each read of the child's output gave: its frames, then
    /// its end or the break of the link.
    frames: mpsc::UnboundedReceiver<Result<Option<Frame>, Broken>>,
    ready_at: Option<Instant>,
}

/// Starts a child, runs its session and ends it, and starts the next
/// one after a pause, until a shutdown.
async fn supervise(
    supervised: &Supervised,
    mut stop: oneshot::Receiver<()>,
) -> Option<ExitStatus> {
    let mut pause = Pause(RESTART);
    loop {
        let steady = match spawn(supervised) {
            Ok(mut child) => {
                let end =
                    session(supervised, &mut child, &mut stop).await;
                let steady = child
                    .ready_at
                    .is_some_and(|ready| ready.elapsed() >= STEADY);
                let status =
                    end_child(&supervised.port, child, &end).await;
                if let End::Shutdown = end {
                    return status;
                }
                steady
            }
            Err(error) => {
                tracing::error!(
                    ?error,
                    argv = ?supervised.argv,
                    "could not start the Node host"
                );
                false
            }
        };
        tokio::select! {
            _ = &mut stop => return None,
            () = tokio::time::sleep(pause.after(steady)) => {}
        }
    }
}

/// The pause before the next child starts.
struct Pause(Duration);

impl Pause {
    fn after(&mut self, steady: bool) -> Duration {
        let pause = if steady { RESTART } else { self.0 };
        self.0 = (pause * 2).min(RESTART_CAP);
        pause
    }
}

fn spawn(supervised: &Supervised) -> io::Result<Running> {
    let (
        tree,
        Pipes {
            input,
            output,
            mut errors,
        },
    ) = Tree::spawn_bound(Command {
        argv: &supervised.argv,
        env: &environment(&supervised.home),
        cwd: &supervised.home,
    })?;
    let out =
        Writer::start(input, "node-link-write", AtBreak::Close)?;
    let (frames, received) = mpsc::unbounded_channel();
    thread::Builder::new()
        .name("node-link-read".to_owned())
        .spawn(move || read_frames(output, &frames))?;
    thread::Builder::new()
        .name("node-link-stderr".to_owned())
        .spawn(move || io::copy(&mut errors, &mut io::stderr()))?;
    Ok(Running {
        tree,
        out,
        frames: received,
        ready_at: None,
    })
}

/// A child's whole environment, from [`ENVIRONMENT`].
fn environment(home: &str) -> BTreeMap<String, String> {
    let mut environment = BTreeMap::new();
    for (name, value) in ENVIRONMENT {
        match value {
            Value::Home => {
                environment
                    .insert((*name).to_owned(), home.to_owned());
            }
            Value::Inherited => {
                if let Ok(value) = std::env::var(name) {
                    environment.insert((*name).to_owned(), value);
                }
            }
            Value::InheritedPrefix => {
                environment.extend(std::env::vars_os().filter_map(
                    |(inherited, value)| {
                        let inherited =
                            inherited.into_string().ok()?;
                        inherited.starts_with(name).then_some((
                            inherited,
                            value.into_string().ok()?,
                        ))
                    },
                ));
            }
        }
    }
    environment
}

/// Sends what each read of a child's `output` gives: each frame, then
/// the output's end or the break of the link.
fn read_frames(
    mut output: PipeReader,
    frames: &mpsc::UnboundedSender<Result<Option<Frame>, Broken>>,
) {
    loop {
        let read = link::read(&mut output);
        let last = !matches!(read, Ok(Some(_)));
        if frames.send(read).is_err() || last {
            return;
        }
    }
}

/// Runs the Rust host's half of the handshake with `child`, then
/// gives the port each frame it writes, until it is lost or a
/// shutdown comes.
async fn session(
    supervised: &Supervised,
    child: &mut Running,
    stop: &mut oneshot::Receiver<()>,
) -> End {
    let port = &supervised.port;
    let mut state = port.state();
    let mut deadline = Some(Instant::now() + HELLO_DEADLINE);
    loop {
        let frame = tokio::select! {
            _ = &mut *stop => return End::Shutdown,
            _ = child.tree.ended() => {
                tracing::warn!("the Node host exited");
                return End::Lost;
            }
            read = child.frames.recv() => match read {
                Some(Ok(Some(frame))) => frame,
                Some(Err(broken)) => {
                    tracing::warn!(%broken, "the link to the Node host broke");
                    return End::Lost;
                }
                Some(Ok(None)) | None => {
                    tracing::warn!("the Node host's output ended");
                    return End::Closed;
                }
            },
            () = until(deadline) => {
                tracing::error!(
                    state = ?*state.borrow(),
                    "the Node host missed its deadline"
                );
                return End::Lost;
            }
        };
        let now = *state.borrow_and_update();
        match (now, frame) {
            (
                LinkState::Down,
                Frame::Hello {
                    protocol,
                    fingerprint,
                    kinds,
                    pid,
                },
            ) => {
                let answer = link::answer(
                    &protocol,
                    &fingerprint,
                    &kinds,
                    port,
                    &supervised.ours,
                    &supervised.routes,
                );
                let written = child.out.write(&answer).await;
                if let Frame::Refused { reason } = &answer {
                    tracing::error!(
                        reason,
                        pid,
                        "refused the Node host's hello"
                    );
                    return End::Closed;
                }
                if let Err(error) = written {
                    tracing::warn!(
                        ?error,
                        "could not welcome the Node host"
                    );
                    return End::Lost;
                }
                port.welcomed(
                    child.out.clone(),
                    supervised.host.clone(),
                );
                deadline = Some(Instant::now() + READY_DEADLINE);
            }
            (LinkState::Welcomed, Frame::Ready) => {
                port.ready();
                child.ready_at = Some(Instant::now());
                deadline = None;
            }
            (LinkState::Down, frame) => {
                tracing::warn!(
                    frame = frame.name(),
                    "the Node host wrote a frame out of turn"
                );
                return End::Lost;
            }
            (LinkState::Welcomed | LinkState::Ready, frame) => {
                if let Err(broken) = port.take(frame) {
                    tracing::warn!(%broken, "the Node host broke the link");
                    return End::Lost;
                }
            }
        }
    }
}

/// Sleeps until `deadline`, or forever when there is none.
async fn until(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

async fn end_child(
    port: &Port,
    mut child: Running,
    end: &End,
) -> Option<ExitStatus> {
    port.down();
    let exited = match end {
        End::Shutdown => tokio::time::timeout(EXIT_DEADLINE, async {
            let _ = child.out.write(&Frame::Goodbye).await;
            child.tree.ended().await
        })
        .await
        .ok(),
        End::Closed => {
            tokio::time::timeout(EXIT_DEADLINE, child.tree.ended())
                .await
                .ok()
        }
        End::Lost => None,
    };
    let ended = match exited {
        Some(ended) => ended,
        None => {
            child.tree.kill();
            child.tree.ended().await
        }
    };
    match ended {
        Ok(status) => {
            tracing::info!(%status, "the Node host ended");
            Some(status)
        }
        Err(error) => {
            tracing::error!(
                ?error,
                "could not wait for the Node host"
            );
            None
        }
    }
}

/// The link's ends in the Node host.
pub(crate) struct Ends {
    pub(crate) frames: Frames,
    /// Frames to the Rust host.
    pub(crate) output: PipeWriter,
}

/// Takes this process's ends of the link to the Rust host that
/// started it. From then on the process is killed when the Rust host
/// is gone or the link's input ends. Fails when this process was not
/// started with [`Tree::spawn_bound`], or has taken the ends already.
pub fn take_link() -> io::Result<()> {
    let process::Ends { input, output } = process::Ends::take()?;
    let (frames, received) = mpsc::unbounded_channel();
    thread::Builder::new()
        .name("link-input".to_owned())
        .spawn(move || read_input(input, &frames))?;
    *ENDS.lock() = Some(Ends {
        frames: Frames(received),
        output,
    });
    Ok(())
}

/// Passes the Rust host's frames on from `input`, and kills the
/// process once the input ends or breaks.
fn read_input(
    mut input: PipeReader,
    frames: &mpsc::UnboundedSender<Frame>,
) -> ! {
    // Reading goes on after the receiver is dropped: only the input's
    // end ends the process.
    while let Ok(Some(frame)) = link::read(&mut input) {
        let _ = frames.send(frame);
    }
    process::end_now()
}

/// The Rust host's frames, which end only with this process.
pub(crate) struct Frames(mpsc::UnboundedReceiver<Frame>);

impl Frames {
    pub(crate) async fn next(&mut self) -> Frame {
        match self.0.recv().await {
            Some(frame) => frame,
            None => std::future::pending().await,
        }
    }
}

pub(crate) fn take_ends() -> Option<Ends> {
    ENDS.lock().take()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pause_doubles_to_a_minute_and_a_steady_child_brings_it_back()
     {
        let mut pause = Pause(RESTART);
        let seconds =
            |pause: &mut Pause, steady| pause.after(steady).as_secs();
        let unsteady: Vec<_> =
            (0..8).map(|_| seconds(&mut pause, false)).collect();
        assert_eq!(unsteady, [1, 2, 4, 8, 16, 32, 60, 60]);
        assert_eq!(seconds(&mut pause, true), 1);
        assert_eq!(seconds(&mut pause, false), 2);
    }
}
