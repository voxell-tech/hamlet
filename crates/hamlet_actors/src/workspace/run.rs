//! A command: its row, which makes a repeat of its call safe, and its
//! process, started under the sandbox on a thread of its own, which
//! lives until the command ends.

use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use hamlet_actor_core::{BindParam, SqliteTransaction};
use hamlet_messages::workspace::reach::Need;
use hamlet_messages::workspace::{
    AgentNumber, CallId, CallRefusal, CommandEnd, CommandId,
    ConversationId, Exit, RunAnswer, Which,
};
use hamlet_os::fs::path::text;
use hamlet_sandbox::scrub::scrub_text;
use hamlet_sandbox::{
    Env, ExecError, Launch, Leftovers, Policy, Sandbox,
};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use self::output::{Output, SPILL_CAP};
use super::calls::{self, Call};

mod output;

/// How long a call waits for its command before it answers `Running`.
pub const ANSWER_WITHIN: Duration = Duration::from_secs(5);

/// The longest a command runs (C16). A call may only shorten it.
pub const LONGEST: Duration = Duration::from_secs(120);

/// How a command's row stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum State {
    Running,
    Ended(CommandEnd),
    /// The workspace's host stopped while it ran.
    CutOff,
    /// It waits for a chief to allow what it needs.
    Waiting(Vec<Need>),
    /// A chief refused it, or its wait lapsed.
    Refused(CallRefusal),
}

/// The op of a command's row.
pub(super) const OP: &str = "run";

/// A command's row.
pub(super) struct Row {
    pub command: CommandId,
    pub state: State,
}

/// The row of `agent`'s command `command`, in `tx`.
pub(super) async fn status(
    tx: &SqliteTransaction,
    agent: AgentNumber,
    command: CommandId,
) -> Result<Option<Row>> {
    let id = i64::try_from(command.0)?;
    calls::by_id(tx, agent, id, OP)
        .await?
        .map(row_of)
        .transpose()
}

/// Marks every command still running as cut off: the host they ran
/// under stopped.
pub(super) const CUT_OFF: &str =
    "UPDATE calls SET state = 'cut_off' WHERE state = 'running'";

/// A command's row, from its call's.
pub(super) fn row_of(call: Call) -> Result<Row> {
    let command = CommandId(u64::try_from(call.id)?);
    let state = match (call.state.as_str(), call.answer) {
        ("running", _) => State::Running,
        ("cut_off", _) => State::CutOff,
        ("waiting", Some(needs)) => State::Waiting(
            ciborium::from_reader(needs.as_slice())
                .context("decode what a command waits for")?,
        ),
        ("refused", Some(refusal)) => State::Refused(
            ciborium::from_reader(refusal.as_slice())
                .context("decode why a command did not run")?,
        ),
        ("ended", Some(answer)) => State::Ended(
            ciborium::from_reader(answer.as_slice())
                .context("decode a command's end")?,
        ),
        (state, _) => {
            bail!("command {} has the state {state}", command.0)
        }
    };
    Ok(Row { command, state })
}

/// Records `agent`'s call `call` as a running command, in `tx`.
pub(super) async fn open(
    tx: &SqliteTransaction,
    agent: AgentNumber,
    call: &CallId,
    request_hash: Vec<u8>,
) -> Result<CommandId> {
    let id = calls::insert(
        tx,
        agent,
        call,
        OP,
        request_hash,
        "running",
        None,
        None,
    )
    .await?;
    Ok(CommandId(u64::try_from(id)?))
}

/// Records how a running command ended, in `tx`.
pub(super) async fn end(
    tx: &SqliteTransaction,
    end: &CommandEnd,
) -> Result<()> {
    let mut answer = Vec::new();
    ciborium::into_writer(end, &mut answer)?;
    tx.execute(
        "UPDATE calls SET state = 'ended', answer = ?1, ended_at = \
         ?2 WHERE id = ?3 AND state = 'running'",
        Some(vec![
            BindParam::Blob(answer),
            BindParam::Integer(calls::now_ms()?),
            BindParam::Integer(i64::try_from(end.command.0)?),
        ]),
    )
    .await?;
    Ok(())
}

/// The end of a command that printed nothing: one that was cut off,
/// or never ran.
pub(super) fn without_output(
    command: CommandId,
    exit: Exit,
) -> CommandEnd {
    CommandEnd {
        command,
        exit,
        output: String::new(),
        cut: None,
        policy: String::new(),
        took_ms: 0,
    }
}

/// A command while it runs, as a cancel finds it.
#[derive(Clone)]
pub(super) struct Running {
    pub command: CommandId,
    pub agent: AgentNumber,
    pub call: CallId,
    pub conversation: ConversationId,
    pub waiters: Arc<Waiters>,
    /// Fired to end it as cancelled.
    pub cancel: CancellationToken,
    /// Fired once its end is recorded, or the host stops.
    pub stopped: CancellationToken,
}

impl Running {
    /// `agent`'s command `command`, started by its call `call` in
    /// `conversation`, whose end `waiters` hear.
    pub(super) fn new(
        command: CommandId,
        agent: AgentNumber,
        call: CallId,
        conversation: ConversationId,
        waiters: Arc<Waiters>,
    ) -> Self {
        Self {
            command,
            agent,
            call,
            conversation,
            waiters,
            cancel: CancellationToken::new(),
            stopped: CancellationToken::new(),
        }
    }

    /// Whether it is a command of `agent` that `which` names.
    pub(super) fn is(
        &self,
        agent: AgentNumber,
        which: &Which,
    ) -> bool {
        self.agent == agent
            && match which {
                Which::One(call) => self.call == *call,
                Which::Conversation(conversation) => {
                    self.conversation == *conversation
                }
                Which::All => true,
            }
    }
}

/// Who hears a running command's end: the call that started it and
/// every repeat of it that came while it waited, or the agent, called
/// with `Finished`. Exactly one of the two hears it.
pub(super) struct Waiters {
    slot: Mutex<Slot>,
    /// When the call that started the command answers `Running`.
    give_up_at: tokio::time::Instant,
}

enum Slot {
    Waiting(Vec<oneshot::Sender<CommandEnd>>),
    /// The calls answered `Running`, so the agent hears the end.
    GaveUp,
    /// The calls heard the end.
    Ended(CommandEnd),
}

impl Waiters {
    pub(super) fn new(within: Duration) -> Arc<Self> {
        Arc::new(Self {
            slot: Mutex::new(Slot::Waiting(Vec::new())),
            give_up_at: tokio::time::Instant::now() + within,
        })
    }

    /// Waiters of a command no call waits for, whose end the agent
    /// hears.
    pub(super) fn given_up() -> Arc<Self> {
        Arc::new(Self {
            slot: Mutex::new(Slot::GaveUp),
            give_up_at: tokio::time::Instant::now(),
        })
    }

    /// Waits for the end of `command` until the first call's wait is
    /// over: `Done` with it, else `Running`.
    pub(super) async fn answer(
        &self,
        command: CommandId,
    ) -> RunAnswer {
        let (tx, rx) = oneshot::channel();
        match &mut *self.lock() {
            Slot::Waiting(waiting) => waiting.push(tx),
            Slot::GaveUp => return RunAnswer::Running { command },
            Slot::Ended(end) => return RunAnswer::Done(end.clone()),
        }
        match tokio::time::timeout_at(self.give_up_at, rx).await {
            Ok(Ok(end)) => RunAnswer::Done(end),
            Ok(Err(_)) => RunAnswer::Running { command },
            Err(_) => {
                let mut slot = self.lock();
                match &*slot {
                    Slot::Ended(end) => RunAnswer::Done(end.clone()),
                    Slot::Waiting(_) | Slot::GaveUp => {
                        *slot = Slot::GaveUp;
                        RunAnswer::Running { command }
                    }
                }
            }
        }
    }

    /// Gives the calls waiting the end, or returns it, for the agent,
    /// when they answered `Running`.
    pub(super) fn ended(
        &self,
        end: CommandEnd,
    ) -> Option<CommandEnd> {
        let mut slot = self.lock();
        match std::mem::replace(&mut *slot, Slot::GaveUp) {
            Slot::Waiting(waiting) => {
                for tx in waiting {
                    let _ = tx.send(end.clone());
                }
                *slot = Slot::Ended(end);
                None
            }
            Slot::GaveUp => Some(end),
            Slot::Ended(first) => {
                *slot = Slot::Ended(first);
                None
            }
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Slot> {
        self.slot.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Everything a command's thread needs.
pub struct Spec {
    pub command: CommandId,
    pub script: String,
    pub cwd: PathBuf,
    pub policy: Policy,
    pub sandbox: Sandbox,
    pub timeout: Duration,
    pub secrets: Arc<Vec<String>>,
    pub leftovers: Leftovers,
    /// Where its output spills.
    pub output: PathBuf,
    /// Fired to end it as cancelled, also before it started.
    pub cancel: CancellationToken,
}

/// Starts `spec`'s command on a thread of its own. The receiver gets
/// its end, or nothing when `abort` fired first, which kills it.
///
/// On Linux the sandbox dies with the thread that started it
/// (`--die-with-parent` is `PR_SET_PDEATHSIG`, which follows the
/// thread), so the command's thread must live until it ends: a
/// thread of the host's runtime can retire while the command runs.
pub fn start(
    spec: Spec,
    abort: CancellationToken,
) -> oneshot::Receiver<CommandEnd> {
    start_on(spec, abort, |name, body| {
        std::thread::Builder::new().name(name).spawn(body).map(drop)
    })
}

/// [`start`], with the thread made by `spawn`.
fn start_on(
    spec: Spec,
    abort: CancellationToken,
    spawn: impl FnOnce(String, Box<dyn FnOnce() + Send>) -> io::Result<()>,
) -> oneshot::Receiver<CommandEnd> {
    let (tx, rx) = oneshot::channel();
    let command = spec.command;
    // Kept here too, so a thread that never runs still ends the call.
    let tx = Arc::new(Mutex::new(Some(tx)));
    let send = |tx: &Mutex<Option<oneshot::Sender<CommandEnd>>>,
                end| {
        if let Some(tx) =
            tx.lock().unwrap_or_else(PoisonError::into_inner).take()
        {
            let _ = tx.send(end);
        }
    };
    let failed = move |why: io::Error| {
        without_output(
            command,
            Exit::Failed(format!(
                "the command could not start: {why}"
            )),
        )
    };
    let thread_tx = tx.clone();
    let spawned = spawn(
        format!("command-{}", command.0),
        Box::new(move || {
            let runtime =
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build();
            let runtime = match runtime {
                Ok(runtime) => runtime,
                Err(error) => return send(&thread_tx, failed(error)),
            };
            runtime.block_on(async {
                tokio::select! {
                    end = ran(spec) => send(&thread_tx, end),
                    () = abort.cancelled() => {}
                }
            });
        }),
    );
    if let Err(error) = spawned {
        send(&tx, failed(error));
    }
    rx
}

/// Runs `spec`'s command to its end.
async fn ran(spec: Spec) -> CommandEnd {
    let started = Instant::now();
    let mut output = Output::new(spec.output, SPILL_CAP);
    let mut policy_hash = String::new();
    let exit = match Env::private(
        &text(&spec.cwd),
        spec.policy,
        spec.sandbox,
    ) {
        Err(error) => Exit::Failed(error.to_string()),
        Ok(env) => {
            policy_hash = env.policy_hash().to_owned();
            let argv =
                ["bash".to_owned(), "-c".to_owned(), spec.script];
            let mut push = |text: &str| output.push(text);
            let streamed = env.stream(
                Launch::Shell(&argv),
                spec.timeout,
                &spec.secrets,
                &spec.leftovers,
                &mut push,
            );
            // A cancel fired before the first poll starts nothing;
            // one fired later drops the stream, which kills the
            // command's group.
            tokio::select! {
                biased;
                () = spec.cancel.cancelled() => Exit::Cancelled,
                ran = streamed => match ran {
                    Ok(code) => Exit::Code(code),
                    Err(ExecError::Timeout { .. }) => Exit::TimedOut,
                    Err(
                        ExecError::Spawn(why) | ExecError::Spill(why),
                    ) => Exit::Failed(why),
                },
            }
        }
    };
    let (tail, cut) = output.finish();
    let (exit, cut) = match (exit, cut) {
        (Exit::Failed(why), cut) => (
            Exit::Failed(scrub_text(&spec.secrets, &why)),
            cut.ok().flatten(),
        ),
        (_, Err(why)) => (Exit::Failed(why), None),
        (exit, Ok(cut)) => (exit, cut),
    };
    CommandEnd {
        command: spec.command,
        exit,
        output: tail,
        cut,
        policy: policy_hash,
        took_ms: u64::try_from(started.elapsed().as_millis())
            .unwrap_or(u64::MAX),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Command 1 running `script` in `cwd`, which it may write.
    fn spec(cwd: &std::path::Path, script: &str) -> Spec {
        Spec {
            command: CommandId(1),
            script: script.to_owned(),
            cwd: cwd.to_owned(),
            policy: Policy {
                rw: vec![cwd.to_owned()],
                env: hamlet_sandbox::base_env(),
                ..Policy::default()
            },
            sandbox: Sandbox::testing(),
            timeout: Duration::from_secs(10),
            secrets: Arc::default(),
            leftovers: hamlet_sandbox::AllLeftovers::default().of(1),
            output: cwd.join("output.log"),
            cancel: CancellationToken::new(),
        }
    }

    /// A command cancelled before its thread starts it never runs,
    /// as when its agent's reach ended between the gate and the
    /// start, and ends as cancelled.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_command_cancelled_before_it_starts_never_runs() {
        let scratch =
            hamlet_testing::Scratch::new("command-cancelled");
        let cwd = scratch.join("work");
        std::fs::create_dir_all(&cwd).unwrap();
        let spec = spec(&cwd, "touch marker");
        spec.cancel.cancel();

        let end = start(spec, CancellationToken::new())
            .await
            .expect("an end");
        tokio::time::sleep(Duration::from_millis(300)).await;

        assert_eq!(end.exit, Exit::Cancelled);
        assert!(!cwd.join("marker").exists(), "the command ran");
    }

    /// A command whose thread cannot be made ends as failed, and
    /// says why, rather than leaving its call to wait for nothing.
    #[tokio::test]
    async fn a_command_without_a_thread_fails() {
        let ended = start_on(
            spec(std::path::Path::new("/"), "true"),
            CancellationToken::new(),
            |_, _| Err(io::Error::other("no threads left")),
        );

        let end = ended.await.expect("an end");
        assert_eq!(
            end.exit,
            Exit::Failed(
                "the command could not start: no threads left".into()
            )
        );
    }

    /// On Linux the sandbox dies with the thread that started it:
    /// a command started from a thread that then exits is killed,
    /// while one whose thread lives reaches its end. So `start` gives
    /// each command a thread of its own. Both runs drive the command
    /// on one runtime, so only the starting thread's exit differs.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_command_dies_with_the_thread_that_started_it() {
        use std::task::{Context, Poll, Waker};

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        let mut seen = Vec::new();
        for thread_exits in [false, true] {
            let scratch =
                hamlet_testing::Scratch::new("command-thread");
            let cwd = scratch.join("work");
            std::fs::create_dir_all(&cwd).unwrap();
            let spec = spec(&cwd, "sleep 1; touch marker");
            let handle = runtime.handle().clone();
            let (moved, arrived) = std::sync::mpsc::channel();
            let (done, release) = std::sync::mpsc::channel::<()>();
            let starter = std::thread::spawn(move || {
                let _entered = handle.enter();
                let mut command = Box::pin(ran(spec));
                // Its first poll starts the sandbox, from this
                // thread.
                let poll = command
                    .as_mut()
                    .poll(&mut Context::from_waker(Waker::noop()));
                assert!(matches!(poll, Poll::Pending));
                moved.send(command).unwrap();
                if thread_exits {
                    // Long enough for the sandbox to tie itself to
                    // this thread, which it does as it starts.
                    std::thread::sleep(Duration::from_millis(500));
                } else {
                    let _ = release.recv();
                }
            });
            let command = arrived.recv().unwrap();
            let end = runtime.block_on(command);
            drop(done);
            starter.join().unwrap();
            seen.push((end.exit, cwd.join("marker").exists()));
        }
        assert_eq!(
            seen,
            [(Exit::Code(0), true), (Exit::Code(128 + 9), false)]
        );
    }
}
