//! The Node host as the Rust host's child process: the Rust host's
//! [`Supervisor`], which starts, watches, ends and restarts it, and
//! the child's own hold on its standard input and output,
//! [`take_stdio`].
//!
//! A child has ended here once it has exited and its process group
//! is killed. tokio reaps its process after that.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, PipeWriter};
use std::process::{ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use hamlet_os::process::{Command, Limits, Tree};
use parking_lot::Mutex;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::Instant;

use crate::link::{self, Broken, Frame};
use crate::route::Routes;

/// The child's variable naming the realm folder it opens.
pub(crate) const HOME: &str = "HAMLET_HOME";

/// The child's variable naming the Rust host's process id.
const PARENT: &str = "HAMLET_LINK_PARENT";

/// The child's whole environment: each variable, and where its value
/// comes from. An inherited variable the Rust host does not have, or
/// whose value is not UTF-8, is left out.
const ENVIRONMENT: &[(&str, Value)] = &[
    (HOME, Value::Home),
    (PARENT, Value::Parent),
    ("PATH", Value::Inherited),
    #[cfg(feature = "faults")]
    ("HAMLET_FAULT_DIR", Value::Inherited),
];

/// How long after a child has ended the next one starts.
const RESTART: Duration = Duration::from_secs(1);

/// How long a child has to say hello, from its start.
const HELLO_DEADLINE: Duration = Duration::from_secs(30);

/// How long a welcomed child has to say it is ready.
const READY_DEADLINE: Duration = Duration::from_secs(120);

/// How long a child has to exit after the goodbye.
const EXIT_DEADLINE: Duration = Duration::from_secs(15);

/// The link's ends in the Node host, from [`take_stdio`] until
/// [`link::serve`] takes them.
static ENDS: Mutex<Option<Ends>> = Mutex::new(None);

/// Where the value of a variable of [`ENVIRONMENT`] comes from.
enum Value {
    /// [`ChildOptions::home`].
    Home,
    /// The Rust host's process id.
    Parent,
    /// The Rust host's own environment.
    Inherited,
}

/// The Node child a [`Supervisor`] keeps running.
#[derive(Clone, Debug)]
pub struct ChildOptions {
    /// The program and its arguments.
    pub argv: Vec<String>,
    /// The realm folder the child opens, and its working folder.
    pub home: String,
    /// The Rust host's own kinds, which its welcome states.
    pub kinds: Vec<String>,
    /// The kinds the child must state in its hello.
    pub node_kinds: Vec<String>,
    /// The route table the welcome gives the child.
    pub routes: Routes,
}

/// The link's state, as the Rust host sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkState {
    /// No child has been welcomed, or the welcomed one is gone.
    Down,
    /// The Rust host has accepted the child's hello.
    Welcomed,
    /// The child has said it is ready.
    Ready,
}

/// Keeps one Node child running, from a thread of its own. A child
/// that exits, breaks the link, misses a deadline of the handshake or
/// is refused is killed with its process group and followed by a new
/// one a second later, for as long as the supervisor runs.
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
    pub fn start(options: ChildOptions) -> io::Result<Self> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let (stop, stopped) = oneshot::channel();
        let (state, watching) = watch::channel(LinkState::Down);
        let logs = tracing::dispatcher::get_default(Clone::clone);
        // One thread spawns every child and lives as long as the
        // supervisor: Linux sends a child its parent-death signal
        // when the thread that started it ends. Its runtime has
        // signals, which a tree waits on.
        let thread = thread::Builder::new()
            .name("node-link".to_owned())
            .spawn(move || {
                let _logs = tracing::dispatcher::set_default(&logs);
                runtime.block_on(supervise(&options, stopped, &state))
            })?;
        Ok(Self {
            stop,
            state: watching,
            thread,
        })
    }

    /// The link's state, which changes only when it moves.
    pub fn state(&self) -> watch::Receiver<LinkState> {
        self.state.clone()
    }

    /// Starts no more children, says goodbye to the running one,
    /// gives it 15 seconds to exit, then kills its process group.
    /// Returns how that child ended, or `None` when no child was
    /// running, as in the second between two children or after a
    /// start that failed, or when its end could not be waited for.
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
    /// The supervisor shuts down.
    Shutdown,
}

/// A child the supervisor started.
struct Running {
    tree: Tree,
    /// Frames to the child.
    input: PipeWriter,
    /// What each read of the child's output gave: its frames, then
    /// its end or the break of the link.
    frames: mpsc::UnboundedReceiver<Result<Option<Frame>, Broken>>,
}

/// Starts a child, runs its session and ends it, and starts the next
/// one a second later, until a shutdown.
async fn supervise(
    options: &ChildOptions,
    mut stop: oneshot::Receiver<()>,
    state: &watch::Sender<LinkState>,
) -> Option<ExitStatus> {
    loop {
        match spawn(options) {
            Ok(mut child) => {
                let end =
                    session(options, &mut child, &mut stop, state)
                        .await;
                let status = end_child(child, &end, state).await;
                if let End::Shutdown = end {
                    return status;
                }
            }
            Err(error) => tracing::error!(
                ?error,
                argv = ?options.argv,
                "could not start the Node host"
            ),
        }
        tokio::select! {
            _ = &mut stop => return None,
            () = tokio::time::sleep(RESTART) => {}
        }
    }
}

/// Starts a child as a tree of its own, with its environment, a
/// thread that reads its frames, and a thread that copies its
/// standard error to the Rust host's until it ends, which outlives
/// the supervisor's runtime.
fn spawn(options: &ChildOptions) -> io::Result<Running> {
    let (reader, input) = io::pipe()?;
    let (tree, stdout, stderr) = Tree::spawn(
        Command {
            argv: &options.argv,
            env: &environment(options),
            cwd: &options.home,
            stdin: Stdio::from(reader),
        },
        &Limits::default(),
    )?;
    #[cfg(unix)]
    let (output, errors) =
        (stdout.into_owned_fd()?, stderr.into_owned_fd()?);
    #[cfg(windows)]
    let (output, errors) =
        (stdout.into_owned_handle()?, stderr.into_owned_handle()?);
    let (output, mut errors) =
        (File::from(output), File::from(errors));
    let (frames, received) = mpsc::unbounded_channel();
    thread::Builder::new()
        .name("node-link-read".to_owned())
        .spawn(move || read_frames(output, &frames))?;
    thread::Builder::new()
        .name("node-link-stderr".to_owned())
        .spawn(move || io::copy(&mut errors, &mut io::stderr()))?;
    Ok(Running {
        tree,
        input,
        frames: received,
    })
}

/// A child's whole environment, from [`ENVIRONMENT`].
fn environment(options: &ChildOptions) -> BTreeMap<String, String> {
    ENVIRONMENT
        .iter()
        .filter_map(|(name, value)| {
            let value = match value {
                Value::Home => options.home.clone(),
                Value::Parent => std::process::id().to_string(),
                Value::Inherited => std::env::var(name).ok()?,
            };
            Some(((*name).to_owned(), value))
        })
        .collect()
}

/// Sends what each read of a child's `output` gives: each frame, then
/// the output's end or the break of the link.
fn read_frames(
    mut output: File,
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
/// watches it until it is lost or a shutdown comes.
async fn session(
    options: &ChildOptions,
    child: &mut Running,
    stop: &mut oneshot::Receiver<()>,
    state: &watch::Sender<LinkState>,
) -> End {
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
                    return End::Lost;
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
        let now = *state.borrow();
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
                    options,
                );
                if let Frame::Refused { reason } = &answer {
                    tracing::error!(
                        reason,
                        pid,
                        "refused the Node host's hello"
                    );
                    let _ = link::write(&mut child.input, &answer);
                    return End::Lost;
                }
                if let Err(error) =
                    link::write(&mut child.input, &answer)
                {
                    tracing::warn!(
                        ?error,
                        "could not welcome the Node host"
                    );
                    return End::Lost;
                }
                set(state, LinkState::Welcomed);
                deadline = Some(Instant::now() + READY_DEADLINE);
            }
            (LinkState::Welcomed, Frame::Ready) => {
                set(state, LinkState::Ready);
                deadline = None;
            }
            (now, frame) => {
                tracing::warn!(
                    state = ?now,
                    ?frame,
                    "the Node host wrote a frame out of turn"
                );
                return End::Lost;
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

/// Ends `child` and returns how it ended. At a shutdown it says
/// goodbye and gives the child [`EXIT_DEADLINE`] to exit; past that,
/// or when the child is lost, it kills the child's tree at once.
/// Dropped, the tree kills the group again, and tokio reaps the
/// child.
async fn end_child(
    mut child: Running,
    end: &End,
    state: &watch::Sender<LinkState>,
) -> Option<ExitStatus> {
    set(state, LinkState::Down);
    let exited = match end {
        End::Shutdown => {
            let _ = link::write(&mut child.input, &Frame::Goodbye);
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

/// Sets the link's state, telling its watchers only when it moves.
fn set(state: &watch::Sender<LinkState>, to: LinkState) {
    state.send_if_modified(|now| std::mem::replace(now, to) != to);
}

/// The link's ends in the Node host.
pub(crate) struct Ends {
    /// The Rust host's frames.
    pub(crate) frames: mpsc::UnboundedReceiver<Frame>,
    /// Frames to the Rust host.
    pub(crate) output: File,
}

/// Takes this process's standard input and output for the link to
/// the Rust host that started it. From then on standard output
/// writes to standard error and standard input reads nothing, here
/// and in the processes it starts, and the process kills itself when
/// the Rust host is gone. Call it first, before anything reads or
/// writes either. Fails when they are taken already, or when
/// `HAMLET_LINK_PARENT` names no process id.
pub fn take_stdio() -> io::Result<()> {
    static TAKEN: AtomicBool = AtomicBool::new(false);
    if TAKEN.swap(true, Ordering::SeqCst) {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "the link has taken standard input and output already",
        ));
    }
    let parent = std::env::var(PARENT)
        .ok()
        .and_then(|pid| pid.parse::<u32>().ok())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{PARENT} names no process id"),
            )
        })?;
    let (input, output) = hamlet_os::process::take_stdio()?;
    let (frames, received) = mpsc::unbounded_channel();
    thread::Builder::new()
        .name("link-input".to_owned())
        .spawn(move || watch_input(parent, input, &frames))?;
    *ENDS.lock() = Some(Ends {
        frames: received,
        output,
    });
    Ok(())
}

/// Passes the Rust host's frames on from `input`, and kills the
/// process once its parent is not `parent` or the input ends or
/// breaks.
fn watch_input(
    parent: u32,
    mut input: File,
    frames: &mpsc::UnboundedSender<Frame>,
) -> ! {
    // On this thread, which lives as long as the process: the
    // parent-death signal holds while the thread that set it lives.
    if hamlet_os::process::end_with_parent(parent).is_err() {
        hamlet_os::process::end_now();
    }
    // Reading goes on after the receiver is dropped: only the input's
    // end ends the process.
    while let Ok(Some(frame)) = link::read(&mut input) {
        let _ = frames.send(frame);
    }
    hamlet_os::process::end_now()
}

/// The link's ends [`take_stdio`] took, once.
pub(crate) fn take_ends() -> Option<Ends> {
    ENDS.lock().take()
}
