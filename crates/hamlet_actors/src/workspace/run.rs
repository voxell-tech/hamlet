//! A command: its row, which makes a repeat of its call safe, and its
//! process, started under the sandbox on a thread of its own, which
//! lives until the command ends.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use hamlet_actor_core::{BindParam, ColumnValue, SqliteTransaction};
use hamlet_messages::workspace::{
    AgentNumber, CallId, CommandEnd, CommandId, Exit, Run, RunAnswer,
};
use hamlet_os::fs::path::text;
use hamlet_sandbox::scrub::scrub_text;
use hamlet_sandbox::{
    Env, ExecError, Launch, Leftovers, Policy, Sandbox,
};
use sha2::{Digest, Sha256};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

/// How long a call waits for its command before it answers `Running`.
pub const ANSWER_WITHIN: Duration = Duration::from_secs(5);

/// The longest a command runs (C16). A call may only shorten it.
pub const LONGEST: Duration = Duration::from_secs(120);

/// The most lines and bytes of a command's output kept (C16).
const TAIL_LINES: usize = 2000;
const TAIL_BYTES: usize = 50 * 1024;

/// One row per call that changes something, by the agent and the
/// id it gave the call. A command's id is its row's.
pub(super) const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS calls (
	id INTEGER PRIMARY KEY AUTOINCREMENT,
	agent INTEGER NOT NULL,
	call TEXT NOT NULL,
	op TEXT NOT NULL,
	request_hash BLOB NOT NULL,
	state TEXT NOT NULL,
	answer BLOB,
	opened_at INTEGER NOT NULL,
	ended_at INTEGER,
	UNIQUE (agent, call)) STRICT";

/// How a command's row stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum State {
    Running,
    Ended(CommandEnd),
    /// The workspace's host stopped while it ran.
    CutOff,
}

/// A command's row.
pub(super) struct Row {
    pub command: CommandId,
    pub request_hash: Vec<u8>,
    pub state: State,
}

/// The hash of `run` that a repeat of its call must match.
pub(super) fn request_hash(run: &Run) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    ciborium::into_writer(run, &mut bytes)?;
    Ok(Sha256::digest(&bytes).to_vec())
}

/// The row of `agent`'s command `command`, in `tx`.
pub(super) async fn status(
    tx: &SqliteTransaction,
    agent: AgentNumber,
    command: CommandId,
) -> Result<Option<Row>> {
    let read = tx
        .execute(
            "SELECT id, request_hash, state, answer FROM calls \
             WHERE agent = ?1 AND id = ?2 AND op = 'run'",
            Some(vec![
                BindParam::Integer(i64::try_from(agent.0)?),
                BindParam::Integer(i64::try_from(command.0)?),
            ]),
        )
        .await?;
    read.rows.first().map(|row| read_row(row)).transpose()
}

/// Marks every command still running as cut off: the host they ran
/// under stopped.
pub(super) const CUT_OFF: &str =
    "UPDATE calls SET state = 'cut_off' WHERE state = 'running'";

/// The row of `agent`'s call `call`, in `tx`.
pub(super) async fn find(
    tx: &SqliteTransaction,
    agent: AgentNumber,
    call: &CallId,
) -> Result<Option<Row>> {
    let read = tx
        .execute(
            "SELECT id, request_hash, state, answer FROM calls \
             WHERE agent = ?1 AND call = ?2",
            Some(vec![
                BindParam::Integer(i64::try_from(agent.0)?),
                BindParam::Text(call.0.clone()),
            ]),
        )
        .await?;
    read.rows.first().map(|row| read_row(row)).transpose()
}

fn read_row(row: &[ColumnValue]) -> Result<Row> {
    let [
        ColumnValue::Integer(id),
        ColumnValue::Blob(request_hash),
        ColumnValue::Text(state),
        answer,
    ] = row
    else {
        bail!("a call's row has another shape: {row:?}");
    };
    let command = CommandId(u64::try_from(*id)?);
    let state = match (state.as_str(), answer) {
        ("running", _) => State::Running,
        ("cut_off", _) => State::CutOff,
        ("ended", ColumnValue::Blob(answer)) => State::Ended(
            ciborium::from_reader(answer.as_slice())
                .context("decode a command's end")?,
        ),
        _ => bail!("command {} has the state {state}", command.0),
    };
    Ok(Row {
        command,
        request_hash: request_hash.clone(),
        state,
    })
}

/// Records `agent`'s call `call` as a running command, in `tx`.
pub(super) async fn open(
    tx: &SqliteTransaction,
    agent: AgentNumber,
    call: &CallId,
    request_hash: Vec<u8>,
) -> Result<CommandId> {
    let opened = tx
        .execute(
            "INSERT INTO calls (agent, call, op, request_hash, \
             state, opened_at) VALUES (?1, ?2, 'run', ?3, \
             'running', ?4)",
            Some(vec![
                BindParam::Integer(i64::try_from(agent.0)?),
                BindParam::Text(call.0.clone()),
                BindParam::Blob(request_hash),
                BindParam::Integer(now_ms()?),
            ]),
        )
        .await?;
    let id = opened
        .last_insert_row_id
        .context("the insert of a command gave no id")?;
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
            BindParam::Integer(now_ms()?),
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
        policy: String::new(),
        took_ms: 0,
    }
}

fn now_ms() -> Result<i64> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?;
    Ok(i64::try_from(now.as_millis())?)
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
}

/// Starts `spec`'s command on a thread of its own. The receiver gets
/// its end, or nothing when `abort` fired first, which kills it, or
/// when the thread could not run it.
///
/// On Linux the sandbox dies with the thread that started it
/// (`--die-with-parent` is `PR_SET_PDEATHSIG`, which follows the
/// thread), so the command's thread must live until it ends: a
/// thread of the host's runtime can retire while the command runs.
pub fn start(
    spec: Spec,
    abort: CancellationToken,
) -> oneshot::Receiver<CommandEnd> {
    let (tx, rx) = oneshot::channel();
    let spawned = std::thread::Builder::new()
        .name(format!("command-{}", spec.command.0))
        .spawn(move || {
            let runtime =
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build();
            let runtime = match runtime {
                Ok(runtime) => runtime,
                Err(error) => {
                    tracing::warn!(%error, "a command's runtime");
                    return;
                }
            };
            runtime.block_on(async {
                tokio::select! {
                    end = ran(spec) => {
                        let _ = tx.send(end);
                    }
                    () = abort.cancelled() => {}
                }
            });
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, "a command's thread");
    }
    rx
}

/// Runs `spec`'s command to its end.
async fn ran(spec: Spec) -> CommandEnd {
    let started = Instant::now();
    let mut tail = Tail::default();
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
            let ran = env
                .exec(
                    Launch::Shell(&argv),
                    spec.timeout,
                    &spec.secrets,
                    &spec.leftovers,
                    &mut |text| tail.push(text),
                )
                .await;
            match ran {
                Ok(ran) => Exit::Code(ran.exit_code),
                Err(ExecError::Timeout { .. }) => Exit::TimedOut,
                Err(
                    ExecError::Spawn(why) | ExecError::Spill(why),
                ) => Exit::Failed(why),
            }
        }
    };
    let exit = match exit {
        Exit::Failed(why) => {
            Exit::Failed(scrub_text(&spec.secrets, &why))
        }
        exit => exit,
    };
    CommandEnd {
        command: spec.command,
        exit,
        output: tail.text(),
        policy: policy_hash,
        took_ms: u64::try_from(started.elapsed().as_millis())
            .unwrap_or(u64::MAX),
    }
}

/// The running output of a command, of which its last lines are kept.
#[derive(Default)]
struct Tail {
    kept: String,
}

impl Tail {
    fn push(&mut self, text: &str) {
        self.kept.push_str(text);
        if self.kept.len() > 4 * TAIL_BYTES {
            self.kept = self.text();
        }
    }

    /// The last [`TAIL_LINES`] lines within [`TAIL_BYTES`], a line
    /// cut by the byte limit dropped whole when a newline follows it,
    /// and without the control characters that break display; tabs
    /// and newlines stay.
    fn text(&self) -> String {
        let kept = self.kept.as_str();
        let body = kept.strip_suffix('\n').unwrap_or(kept);
        let mut start = body
            .rmatch_indices('\n')
            .nth(TAIL_LINES - 1)
            .map_or(0, |(at, _)| at + 1);
        if kept.len() - start > TAIL_BYTES {
            let mut from = kept.len() - TAIL_BYTES;
            while !kept.is_char_boundary(from) {
                from += 1;
            }
            start = match kept[from..].find('\n') {
                Some(at) if from + at + 1 < kept.len() => {
                    from + at + 1
                }
                _ => from,
            };
        }
        kept[start..]
            .chars()
            .filter(|&c| {
                !matches!(
                    c,
                    '\0'..='\u{8}' | '\u{b}'..='\u{1f}' | '\u{fff9}'..='\u{fffb}'
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            let spec = Spec {
                command: CommandId(1),
                script: "sleep 1; touch marker".to_owned(),
                cwd: cwd.clone(),
                policy: Policy {
                    rw: vec![cwd.clone()],
                    env: hamlet_sandbox::base_env(),
                    ..Policy::default()
                },
                sandbox: Sandbox::testing(),
                timeout: Duration::from_secs(10),
                secrets: Arc::default(),
                leftovers: hamlet_sandbox::AllLeftovers::default()
                    .of(1),
            };
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

    #[test]
    fn the_tail_keeps_the_last_lines_within_the_limits() {
        let mut tail = Tail::default();
        for n in 0..300_000 {
            tail.push(&format!("{n}\n"));
        }
        let kept = tail.text();
        assert!(kept.starts_with("298000\n"), "{}", &kept[..20]);
        assert!(kept.ends_with("299999\n"));
        let mut wide = Tail::default();
        wide.push(&format!("{}\nend", "x".repeat(60 * 1024)));
        assert_eq!(wide.text(), "end");
        let mut long = Tail::default();
        long.push(&"\u{e9}".repeat(30 * 1024));
        assert_eq!(long.text().len(), TAIL_BYTES);
    }
}
