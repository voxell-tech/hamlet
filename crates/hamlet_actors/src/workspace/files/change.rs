//! A write or an edit of a project file. One change at a time takes
//! a path. Its call's row is written before the change, with the new
//! file it is about to make, and records the answer after it. The new
//! file replaces the old one only while the old one holds what the
//! change read. A change a kill cut off is gone: its new file and its
//! row are removed, and a repeat is a fresh call.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use anyhow::{Context, Result};
use hamlet_actor::{Ctx, Handler};
use hamlet_actor_core::{BindParam, ColumnValue, SqliteTransaction};
use hamlet_messages::workspace::files::{
    Change, ChangeAnswer, EditDiff, EditFile, Hashed, MOST_CONTENT,
    Replace, WriteFile,
};
use hamlet_messages::workspace::{
    AgentNumber, CallId, CallRefusal, ConversationId,
};
use hamlet_os::fs::path::{self, is_absolute, normalize, text};
use hamlet_sandbox::scrub::scrub_text;
use hamlet_sandbox::{Current, Env, FileError, Replaced};
use hamlet_workspace_wire::Code;
use sha2::{Digest, Sha256};
use tokio::sync::OwnedMutexGuard;

use super::edit::{self, CallError};
use super::{Gated, Passed};
use crate::workspace::reach::{self, Op, Placement};
use crate::workspace::{
    Workspace, agent_of, caller, calls, checked_call, refused,
};

/// The paths changes are at work on: one change at a time checks and
/// replaces a path.
#[derive(Default)]
pub struct Paths(Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>);

impl Paths {
    /// Waits until no other change is at work on `path`, and holds it
    /// until the guard is dropped.
    ///
    /// The lock is by the path as written. A change through a link
    /// and one through its target take two locks, so both can pass
    /// the check of what the file holds before either renames, and
    /// the second rename wins.
    async fn hold(&self, path: &str) -> OwnedMutexGuard<()> {
        let lock = {
            let mut paths =
                self.0.lock().unwrap_or_else(PoisonError::into_inner);
            paths.retain(|_, lock| Arc::strong_count(lock) > 1);
            paths.entry(normalize(path)).or_default().clone()
        };
        lock.lock_owned().await
    }
}

/// Removes the new files of the changes a kill cut off, and their
/// rows, so that a repeat of each is a fresh call.
pub async fn cut_off_changes(ctx: &Ctx<Workspace>) -> Result<()> {
    let tx = ctx.sql().begin_transaction(None).await?;
    let unfinished = tx
        .execute(
            "SELECT id, temp FROM calls WHERE state = 'changing'",
            None,
        )
        .await;
    let unfinished = match unfinished {
        Ok(read) => read.rows,
        Err(error) => {
            tx.rollback().await?;
            return Err(error);
        }
    };
    for row in unfinished {
        let [ColumnValue::Integer(id), temp] = row.as_slice() else {
            continue;
        };
        if let ColumnValue::Text(temp) = temp {
            remove_new_file(temp);
        }
        if let Err(error) = forget(&tx, *id).await {
            tx.rollback().await?;
            return Err(error);
        }
    }
    tx.commit().await
}

/// Removes the new file a change was about to make. A failure is
/// logged, and the change is forgotten all the same.
fn remove_new_file(temp: &str) {
    match std::fs::remove_file(temp) {
        Ok(()) => {}
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => tracing::warn!(
            ?error,
            temp,
            "remove the new file of a change that was cut off"
        ),
    }
}

async fn forget(tx: &SqliteTransaction, id: i64) -> Result<()> {
    tx.execute(
        "DELETE FROM calls WHERE id = ?1",
        Some(vec![BindParam::Integer(id)]),
    )
    .await?;
    Ok(())
}

/// What a change asks for.
enum Asked<'a> {
    Write(&'a str),
    Edit(&'a [Replace]),
}

impl Asked<'_> {
    fn op(&self) -> &'static str {
        match self {
            Self::Write(_) => "write",
            Self::Edit(_) => "edit",
        }
    }
}

/// A change as the handler weighs it.
struct Request<'a> {
    named: AgentNumber,
    call: &'a CallId,
    conversation: &'a ConversationId,
    path: &'a Path,
    asked: Asked<'a>,
    request_hash: Vec<u8>,
}

impl Handler<WriteFile> for Workspace {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        asked: WriteFile,
    ) -> Result<ChangeAnswer> {
        self.change(
            &ctx,
            Request {
                named: asked.agent,
                call: &asked.call,
                conversation: &asked.conversation,
                path: &asked.path,
                asked: Asked::Write(&asked.content),
                request_hash: calls::request_hash(&asked)?,
            },
        )
        .await
    }
}

impl Handler<EditFile> for Workspace {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        asked: EditFile,
    ) -> Result<ChangeAnswer> {
        self.change(
            &ctx,
            Request {
                named: asked.agent,
                call: &asked.call,
                conversation: &asked.conversation,
                path: &asked.path,
                asked: Asked::Edit(&asked.edits),
                request_hash: calls::request_hash(&asked)?,
            },
        )
        .await
    }
}

/// What a change's row, its reach and its secrets come to.
enum Weighed {
    Answer(ChangeAnswer),
    /// A change a failure cut off after its row was written, which no
    /// one is at work on.
    Unfinished {
        id: i64,
        temp: Option<String>,
    },
    Fetch(hamlet_messages::workspace::reach::SecretsVersion),
    Pass(Passed),
}

/// The file a change read, and what it would write over it.
struct Planned {
    current: Current,
    content: Vec<u8>,
    diff: Option<EditDiff>,
}

impl Workspace {
    async fn change(
        &self,
        ctx: &Ctx<Self>,
        request: Request<'_>,
    ) -> Result<ChangeAnswer> {
        let action = request.asked.op();
        let caller = caller(ctx)?;
        let agent = match agent_of(caller.as_ref(), request.named) {
            Ok(agent) => agent,
            Err(refusal) => {
                refused(caller.as_ref(), action, &refusal);
                return Ok(ChangeAnswer::Refused(
                    CallRefusal::Caller(refusal),
                ));
            }
        };
        let path = text(request.path);
        if let Err(why) = checked_call(request.call).and_then(|()| {
            is_absolute(&path).then_some(()).ok_or_else(|| {
                format!("{path} is not an absolute path")
            })
        }) {
            return Ok(ChangeAnswer::Refused(CallRefusal::Invalid(
                why,
            )));
        }
        if let Asked::Write(content) = request.asked {
            let bytes = u64::try_from(content.len())?;
            if bytes > MOST_CONTENT {
                return Ok(ChangeAnswer::Refused(
                    CallRefusal::TooLarge {
                        bytes,
                        limit: MOST_CONTENT,
                    },
                ));
            }
        }
        let held = self.paths.hold(&path).await;
        let mut fetched = false;
        let passed = loop {
            match self.weigh(ctx, agent, &request).await? {
                Weighed::Answer(answer) => return Ok(answer),
                // Also the row of a first copy that was dropped
                // between its rename and its record: this copy then
                // runs afresh on the file the first one changed, so
                // an edit can answer that its old text was not found
                // for a change that was made.
                Weighed::Unfinished { id, temp } => {
                    if let Some(temp) = temp {
                        remove_new_file(&temp);
                    }
                    let tx =
                        ctx.sql().begin_transaction(None).await?;
                    if let Err(error) = forget(&tx, id).await {
                        tx.rollback().await?;
                        return Err(error);
                    }
                    tx.commit().await?;
                }
                Weighed::Fetch(version) => {
                    self.fetch_secrets(ctx, version, &mut fetched)
                        .await?;
                }
                Weighed::Pass(passed) => break passed,
            }
        };
        let Passed {
            placement,
            sandbox,
            secrets,
        } = passed;
        let planned = {
            let placement = placement.clone();
            let path = path.clone();
            let asked = match request.asked {
                Asked::Write(content) => {
                    PlannedAs::Write(content.as_bytes().to_vec())
                }
                Asked::Edit(edits) => PlannedAs::Edit(edits.to_vec()),
            };
            tokio::task::spawn_blocking(move || {
                plan(&placement, sandbox, &path, asked)
            })
            .await
            .context("a change's thread ended before it")?
        };
        let planned = match planned {
            Ok(planned) => planned,
            Err(words) => {
                return Ok(ChangeAnswer::Failed(scrub_text(
                    &secrets, &words,
                )));
            }
        };
        let temp_name =
            format!(".hamlet-{}.new", uuid::Uuid::new_v4());
        let temp = path::join(&[
            path::dirname(&planned.current.real).as_str(),
            &temp_name,
        ]);
        let tx = ctx.sql().begin_transaction(None).await?;
        let opened = open(&tx, agent, &request, &temp).await;
        let id = match opened {
            Ok(Ok(id)) => {
                tx.commit().await?;
                id
            }
            Ok(Err(answer)) => {
                tx.rollback().await?;
                return Ok(answer);
            }
            Err(error) => {
                tx.rollback().await?;
                return Err(error);
            }
        };
        // The guard goes with the replace, so the path stays held
        // until it is done even if this call is dropped meanwhile.
        let (replaced, held, planned) = {
            let path = path.clone();
            tokio::task::spawn_blocking(move || {
                let replaced =
                    env(&placement, sandbox).and_then(|env| {
                        carried_out(&env, &path, &temp_name, &planned)
                    });
                (replaced, held, planned)
            })
            .await
            .context("a change's thread ended before it")?
        };
        let answer = scrubbed(
            &secrets,
            outcome(request.path, &request.asked, planned, replaced),
        );
        let tx = ctx.sql().begin_transaction(None).await?;
        if let Err(error) = end(&tx, id, &answer).await {
            tx.rollback().await?;
            return Err(error);
        }
        tx.commit().await?;
        drop(held);
        Ok(answer)
    }

    /// Answers a repeat of `request` from its row, or passes it
    /// through the gate with `agent`'s reach.
    async fn weigh(
        &self,
        ctx: &Ctx<Self>,
        agent: AgentNumber,
        request: &Request<'_>,
    ) -> Result<Weighed> {
        let tx = ctx.sql().begin_transaction(None).await?;
        let found = calls::find(&tx, agent, request.call).await;
        let reach = reach::read(&tx, agent).await;
        tx.rollback().await?;
        if let Some(call) = found? {
            return answered(call, request);
        }
        let op = Op::Change {
            conversation: request.conversation,
            path: request.path,
        };
        Ok(match self.gated(agent, reach?.as_ref(), &op)? {
            Gated::Pass(passed) => Weighed::Pass(passed),
            Gated::Refused(refusal) => {
                Weighed::Answer(ChangeAnswer::Refused(refusal))
            }
            Gated::Fetch(version) => Weighed::Fetch(version),
        })
    }
}

/// What the row `call` answers a change of `request`.
fn answered(
    call: calls::Call,
    request: &Request<'_>,
) -> Result<Weighed> {
    if !call.is(request.asked.op(), &request.request_hash) {
        return Ok(Weighed::Answer(ChangeAnswer::Refused(
            CallRefusal::CallReused(request.call.clone()),
        )));
    }
    Ok(match (call.state.as_str(), call.answer) {
        ("ended", Some(answer)) => Weighed::Answer(
            ciborium::from_reader(answer.as_slice())
                .context("decode a change's answer")?,
        ),
        _ => Weighed::Unfinished {
            id: call.id,
            temp: call.temp,
        },
    })
}

/// What a change plans to write, copied out of the request for its
/// thread.
enum PlannedAs {
    Write(Vec<u8>),
    Edit(Vec<Replace>),
}

/// Reads what the change at `path` replaces, and works out what it
/// writes, or says in words why it cannot.
fn plan(
    placement: &Placement,
    sandbox: hamlet_sandbox::Sandbox,
    path: &str,
    asked: PlannedAs,
) -> Result<Planned, String> {
    let current = env(placement, sandbox)
        .and_then(|env| env.read_for_change(path));
    match asked {
        PlannedAs::Write(content) => Ok(Planned {
            current: current.map_err(|error| error.message)?,
            content,
            diff: None,
        }),
        PlannedAs::Edit(edits) => {
            let current = current.map_err(|error| {
                match error.code {
                    // A folder, or another file that is not regular.
                    Code::IsDirectory | Code::Invalid => {
                        CallError::NotAFile(path.to_owned())
                    }
                    code => CallError::Unedited {
                        path: path.to_owned(),
                        code,
                    },
                }
                .to_string()
            })?;
            let (content, diff) =
                edit::edited(path, current.bytes.clone(), &edits)
                    .map_err(|error| error.to_string())?;
            Ok(Planned {
                current,
                content: content.into_bytes(),
                diff: Some(diff),
            })
        }
    }
}

/// Replaces the file at `path` with what `planned` writes, through
/// the new file `temp_name`, only while it holds what `planned` read.
fn carried_out(
    env: &Env,
    path: &str,
    temp_name: &str,
    planned: &Planned,
) -> Result<Replaced, FileError> {
    env.replace_file(
        path,
        &planned.current.real,
        temp_name,
        planned.current.bytes.as_deref(),
        &planned.content,
    )
}

/// The file tools' environment of `placement`, failing in a file's
/// words.
fn env(
    placement: &Placement,
    sandbox: hamlet_sandbox::Sandbox,
) -> Result<Env, FileError> {
    Env::new(&text(&placement.cwd), placement.policy.clone(), sandbox)
        .map_err(|error| FileError {
            code: Code::Unknown,
            message: error.to_string(),
        })
}

/// Records `request` of `agent` as a change at work that is about to
/// make the new file `temp`, in `tx`, or answers it from the row a
/// change with its id wrote meanwhile.
async fn open(
    tx: &SqliteTransaction,
    agent: AgentNumber,
    request: &Request<'_>,
    temp: &str,
) -> Result<Result<i64, ChangeAnswer>> {
    if let Some(call) = calls::find(tx, agent, request.call).await? {
        return Ok(Err(match answered(call, request)? {
            Weighed::Answer(answer) => answer,
            _ => ChangeAnswer::Failed(
                "another change with this call id is at work".into(),
            ),
        }));
    }
    let opened = tx
        .execute(
            "INSERT INTO calls (agent, call, op, request_hash, \
             state, temp, opened_at) VALUES (?1, ?2, ?3, ?4, \
             'changing', ?5, ?6)",
            Some(vec![
                BindParam::Integer(i64::try_from(agent.0)?),
                BindParam::Text(request.call.0.clone()),
                BindParam::Text(request.asked.op().to_owned()),
                BindParam::Blob(request.request_hash.clone()),
                BindParam::Text(temp.to_owned()),
                BindParam::Integer(calls::now_ms()?),
            ]),
        )
        .await?;
    Ok(Ok(opened
        .last_insert_row_id
        .context("the insert of a change gave no id")?))
}

/// Records the answer of the change `id`, in `tx`.
async fn end(
    tx: &SqliteTransaction,
    id: i64,
    answer: &ChangeAnswer,
) -> Result<()> {
    let mut bytes = Vec::new();
    ciborium::into_writer(answer, &mut bytes)?;
    tx.execute(
        "UPDATE calls SET state = 'ended', answer = ?1, temp = \
         NULL, ended_at = ?2 WHERE id = ?3",
        Some(vec![
            BindParam::Blob(bytes),
            BindParam::Integer(calls::now_ms()?),
            BindParam::Integer(id),
        ]),
    )
    .await?;
    Ok(())
}

/// The answer of a change of `path` that planned `planned` and was
/// `replaced`.
fn outcome(
    path: &Path,
    asked: &Asked<'_>,
    planned: Planned,
    replaced: Result<Replaced, FileError>,
) -> ChangeAnswer {
    let hashed = |bytes: Option<&[u8]>| {
        bytes.map_or(Hashed::Absent, |bytes| {
            Hashed::Sha256(Sha256::digest(bytes).into())
        })
    };
    match replaced {
        Ok(Replaced::Done { created }) => {
            ChangeAnswer::Changed(Change {
                path: path.to_owned(),
                created,
                hash: Sha256::digest(&planned.content).into(),
                diff: planned.diff,
            })
        }
        Ok(Replaced::Changed(now)) => {
            ChangeAnswer::Refused(CallRefusal::FileChanged {
                path: path.to_owned(),
                was: hashed(planned.current.bytes.as_deref()),
                now: hashed(now.as_deref()),
            })
        }
        Err(error) => ChangeAnswer::Failed(match asked {
            Asked::Write(_) => error.message,
            Asked::Edit(_) => CallError::Unedited {
                path: text(path),
                code: error.code,
            }
            .to_string(),
        }),
    }
}

/// `answer` with `secrets` taken out of every word, as from every
/// tool's result today (C32).
fn scrubbed(
    secrets: &[String],
    answer: ChangeAnswer,
) -> ChangeAnswer {
    let scrub = |words: &str| scrub_text(secrets, words);
    match answer {
        ChangeAnswer::Changed(mut change) => {
            if let Some(diff) = &mut change.diff {
                diff.diff = scrub(&diff.diff);
                diff.patch = scrub(&diff.patch);
            }
            ChangeAnswer::Changed(change)
        }
        ChangeAnswer::Failed(words) => {
            ChangeAnswer::Failed(scrub(&words))
        }
        refused @ ChangeAnswer::Refused(_) => refused,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn planned(was: Option<&[u8]>) -> Planned {
        Planned {
            current: Current {
                real: "/p/a.md".into(),
                bytes: was.map(<[u8]>::to_vec),
            },
            content: b"new".to_vec(),
            diff: None,
        }
    }

    /// A change's row answers a repeat of it once it ended. One that
    /// did not end, as a failure after its row leaves it, is cut off
    /// so the repeat runs afresh; a row of another request refuses
    /// it.
    #[test]
    fn a_row_answers_a_repeat_or_is_cut_off() {
        let call = CallId("w1".into());
        let conversation = ConversationId("c1".into());
        let request = Request {
            named: AgentNumber(7),
            call: &call,
            conversation: &conversation,
            path: Path::new("/p/a.md"),
            asked: Asked::Write("new"),
            request_hash: vec![1],
        };
        let answer = ChangeAnswer::Failed("no".into());
        let mut bytes = Vec::new();
        ciborium::into_writer(&answer, &mut bytes).unwrap();
        let row =
            |op: &str, hash: u8, state: &str, answer| calls::Call {
                id: 4,
                op: op.into(),
                request_hash: vec![hash],
                state: state.into(),
                answer,
                temp: Some("/p/.hamlet-x.new".into()),
            };

        let ended =
            answered(row("write", 1, "ended", Some(bytes)), &request);
        let unfinished =
            answered(row("write", 1, "changing", None), &request);
        let other =
            answered(row("write", 2, "changing", None), &request);
        let a_run =
            answered(row("run", 1, "running", None), &request);

        assert!(matches!(
            ended.unwrap(),
            Weighed::Answer(given) if given == answer
        ));
        assert!(matches!(
            unfinished.unwrap(),
            Weighed::Unfinished { id: 4, temp: Some(temp) }
                if temp == "/p/.hamlet-x.new"
        ));
        for refused in [other, a_run] {
            assert!(matches!(
                refused.unwrap(),
                Weighed::Answer(ChangeAnswer::Refused(
                    CallRefusal::CallReused(_)
                ))
            ));
        }
    }

    /// A change replaces the file only while it holds what the change
    /// read: a file written meanwhile is left as it is, and the
    /// change hears what it holds now.
    #[test]
    fn a_file_written_after_the_read_is_left_as_it_is() {
        let scratch =
            hamlet_testing::Scratch::new("change-meanwhile");
        let cwd = scratch.join("work");
        std::fs::create_dir_all(&cwd).unwrap();
        let file = text(&cwd.join("a.md"));
        std::fs::write(&file, "old").unwrap();
        let env = Env::new(
            &text(&cwd),
            hamlet_sandbox::Policy {
                rw: vec![cwd.clone()],
                env: hamlet_sandbox::base_env(),
                ..hamlet_sandbox::Policy::default()
            },
            hamlet_sandbox::Sandbox::testing(),
        )
        .unwrap();
        let planned = Planned {
            current: env.read_for_change(&file).unwrap(),
            content: b"new".to_vec(),
            diff: None,
        };
        std::fs::write(&file, "theirs").unwrap();

        let replaced = carried_out(&env, &file, ".a.new", &planned);

        assert_eq!(
            replaced.unwrap(),
            Replaced::Changed(Some(b"theirs".to_vec()))
        );
        assert_eq!(std::fs::read(&file).unwrap(), b"theirs");
    }

    /// A file that no longer held what the change read is refused
    /// with what it held and what it holds.
    #[test]
    fn a_file_that_changed_is_refused_with_both_hashes() {
        let refused = outcome(
            Path::new("/p/a.md"),
            &Asked::Write("new"),
            planned(Some(b"old")),
            Ok(Replaced::Changed(Some(b"theirs".to_vec()))),
        );
        let gone = outcome(
            Path::new("/p/a.md"),
            &Asked::Write("new"),
            planned(Some(b"old")),
            Ok(Replaced::Changed(None)),
        );

        let old = Hashed::Sha256(Sha256::digest(b"old").into());
        assert_eq!(
            refused,
            ChangeAnswer::Refused(CallRefusal::FileChanged {
                path: "/p/a.md".into(),
                was: old,
                now: Hashed::Sha256(Sha256::digest(b"theirs").into()),
            })
        );
        assert_eq!(
            gone,
            ChangeAnswer::Refused(CallRefusal::FileChanged {
                path: "/p/a.md".into(),
                was: old,
                now: Hashed::Absent,
            })
        );
    }

    /// A replace that failed answers a write in the file's words, and
    /// an edit in today's words for an edit.
    #[test]
    fn a_failed_replace_says_why() {
        let error = || FileError {
            code: Code::PermissionDenied,
            message: "EACCES: permission denied, open '/p/a.md'"
                .into(),
        };
        let wrote = outcome(
            Path::new("/p/a.md"),
            &Asked::Write("new"),
            planned(None),
            Err(error()),
        );
        let edited = outcome(
            Path::new("/p/a.md"),
            &Asked::Edit(&[]),
            planned(None),
            Err(error()),
        );

        assert_eq!(
            wrote,
            ChangeAnswer::Failed(
                "EACCES: permission denied, open '/p/a.md'".into()
            )
        );
        assert_eq!(
            edited,
            ChangeAnswer::Failed(
                "Could not edit file: /p/a.md. Error code: \
                 permission_denied."
                    .into()
            )
        );
    }
}
