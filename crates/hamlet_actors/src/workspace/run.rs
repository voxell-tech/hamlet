//! A command: its row, which makes a repeat of its call safe, and its
//! process, started under the sandbox on a thread of its own, which
//! lives until the command ends.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use hamlet_actor_core::{BindParam, ColumnValue, SqliteTransaction};
use hamlet_messages::workspace::{
    AgentNumber, CallId, CommandEnd, CommandId, Exit, Run,
};
use hamlet_os::fs::path::text;
use hamlet_sandbox::scrub::scrub_text;
use hamlet_sandbox::{
    Env, ExecError, Launch, Leftovers, Policy, Sandbox,
};
use sha2::{Digest, Sha256};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

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

/// The end the agent hears for a command that was cut off.
pub(super) fn cut_off(command: CommandId) -> CommandEnd {
    CommandEnd {
        command,
        exit: Exit::CutOff,
        output: String::new(),
        policy: String::new(),
        took_ms: 0,
    }
}

fn now_ms() -> Result<i64> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?;
    Ok(i64::try_from(now.as_millis())?)
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
/// its end, or nothing when `abort` fired first, which kills it.
///
/// On Linux the sandbox dies with the thread that started it
/// (`--die-with-parent` is `PR_SET_PDEATHSIG`, which follows the
/// thread), so the command's thread must live until it ends: a
/// thread of the host's runtime can retire while the command runs.
pub fn start(
    spec: Spec,
    abort: CancellationToken,
) -> std::io::Result<oneshot::Receiver<CommandEnd>> {
    let (tx, rx) = oneshot::channel();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    std::thread::Builder::new()
        .name(format!("command-{}", spec.command.0))
        .spawn(move || {
            runtime.block_on(async {
                tokio::select! {
                    end = ran(spec) => {
                        let _ = tx.send(end);
                    }
                    () = abort.cancelled() => {}
                }
            });
        })?;
    Ok(rx)
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
