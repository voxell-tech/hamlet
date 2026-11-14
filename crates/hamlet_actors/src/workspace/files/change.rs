//! A write or an edit of a project file. One change at a time takes
//! a path. Its call's row is written before the change, with the new
//! file it is about to make, and records the answer after it. The new
//! file replaces the old one only while the old one holds what the
//! change read. A change a kill cut off is gone: its new file and its
//! row are removed, and a repeat is a fresh call.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use anyhow::{Context, Result, bail};
use hamlet_actor::{Action as _, Ctx, Handler};
use hamlet_actor_core::{BindParam, ColumnValue, SqliteTransaction};
use hamlet_messages::address::Agent;
use hamlet_messages::agent::Outcome;
use hamlet_messages::bytes;
use hamlet_messages::sender::{id_reused, sender};
use hamlet_messages::workspace::files::{
    Change, ChangeAnswer, ChangeEnd, EditDiff, EditFile, Hashed,
    MOST_CONTENT, Replace, WriteFile,
};
use hamlet_messages::workspace::reach::Need;
use hamlet_messages::workspace::refusal::{
    CallRefusal, ChangeRefusal, FileGate, InvalidContent,
    InvalidPath, PathRefusal, ReachRefusal, Stale, ToChange,
    WriteRefusal,
};
use hamlet_messages::workspace::{
    AgentNumber, AnswerTaken, CallId, ConversationId, ReachVersion,
};
use hamlet_os::fs::path::{self, is_absolute, normalize, text};
use hamlet_sandbox::scrub::scrub_text;
use hamlet_sandbox::{Current, Env, FileError, Replaced};
use hamlet_workspace_wire::Code;
use sha2::{Digest, Sha256};
use tokio::sync::OwnedMutexGuard;

use self::shown::cut;
use super::edit::{self, CallError};
use super::{Gated, Passed};
use crate::workspace::pending::{
    self, NotRun, Pending, Rechecked, Wait,
};
use crate::workspace::reach::{
    self, Op, Placement, Purpose, Refused,
};
use crate::workspace::{
    Weighing, Workspace, calls, checked_call, read_only, tell,
    transact, transact_if,
};

mod shown;

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
    transact(ctx, async |tx| {
        let unfinished = tx
            .execute(
                "SELECT id, temp FROM calls WHERE state = 'changing'",
                None,
            )
            .await?
            .rows;
        for row in unfinished {
            let [ColumnValue::Integer(id), temp] = row.as_slice()
            else {
                continue;
            };
            if let ColumnValue::Text(temp) = temp {
                remove_new_file(temp);
            }
            forget(tx, *id).await?;
        }
        Ok(())
    })
    .await
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

/// The ops of a write and an edit, as a call's row holds them.
pub(in crate::workspace) const WRITE: &str = "write";
pub(in crate::workspace) const EDIT: &str = "edit";

enum Asked<'a> {
    Write(&'a str),
    Edit(&'a [Replace]),
}

impl Asked<'_> {
    fn op(&self) -> &'static str {
        match self {
            Self::Write(_) => WRITE,
            Self::Edit(_) => EDIT,
        }
    }

    /// The name of the action that asked for it.
    fn action(&self) -> &'static str {
        match self {
            Self::Write(_) => WriteFile::NAME,
            Self::Edit(_) => EditFile::NAME,
        }
    }
}

/// What a fresh change is refused with: a write's or an edit's.
trait Refusal:
    From<CallRefusal>
    + From<InvalidPath>
    + From<PathRefusal<ToChange>>
    + From<ReachRefusal>
{
}

impl Refusal for WriteRefusal {}
impl Refusal for ChangeRefusal {}

/// A change as the handler weighs it.
struct Request<'a> {
    call: &'a CallId,
    conversation: &'a ConversationId,
    path: &'a Path,
    asked: Asked<'a>,
    request_hash: Vec<u8>,
}

impl<'a> Request<'a> {
    /// The change `pending` asks for.
    fn of(pending: &'a Pending) -> Result<Self> {
        Ok(match pending {
            Pending::Write(asked) => Self {
                call: &asked.call,
                conversation: &asked.conversation,
                path: &asked.path,
                asked: Asked::Write(&asked.content),
                request_hash: calls::request_hash(asked)?,
            },
            Pending::Edit(asked) => Self {
                call: &asked.call,
                conversation: &asked.conversation,
                path: &asked.path,
                asked: Asked::Edit(&asked.edits),
                request_hash: calls::request_hash(asked)?,
            },
            Pending::Run(_) => bail!("a command is not a change"),
        })
    }

    /// What the change is refused with before anything is kept of
    /// it: its call id, then its path.
    fn checked<R: Refusal>(&self) -> Option<R> {
        if let Some(refusal) = checked_call(self.call) {
            return Some(refusal.into());
        }
        (!is_absolute(&text(self.path))).then(|| {
            InvalidPath::NotAbsolute {
                path: self.path.to_owned(),
            }
            .into()
        })
    }
}

impl Handler<WriteFile> for Workspace {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        asked: WriteFile,
    ) -> Result<ChangeAnswer<WriteRefusal>> {
        let Agent(agent) = sender::<WriteFile, _>(&ctx)?;
        let bytes = u64::try_from(asked.content.len())?;
        let pending = Pending::Write(asked);
        let request = Request::of(&pending)?;
        if let Some(why) = request.checked::<WriteRefusal>() {
            return Ok(ChangeAnswer::Refused { why });
        }
        if bytes > MOST_CONTENT {
            let why = InvalidContent::TooLarge {
                bytes,
                limit: MOST_CONTENT,
            };
            return Ok(ChangeAnswer::Refused { why: why.into() });
        }
        self.change(&ctx, agent, &pending, &request).await
    }
}

impl Handler<EditFile> for Workspace {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        asked: EditFile,
    ) -> Result<ChangeAnswer> {
        let Agent(agent) = sender::<EditFile, _>(&ctx)?;
        let pending = Pending::Edit(asked);
        let request = Request::of(&pending)?;
        if let Some(why) = request.checked::<ChangeRefusal>() {
            return Ok(ChangeAnswer::Refused { why });
        }
        self.change(&ctx, agent, &pending, &request).await
    }
}

enum Weighed<R> {
    Answer(ChangeAnswer<R>),
    Pass(Passed),
    /// It waits for a chief to allow these, under this reach.
    Chief(Passed, Vec<Need>, ReachVersion),
}

/// The file a change read, and what it would write over it.
struct Planned {
    current: Current,
    content: Vec<u8>,
    diff: Option<EditDiff>,
    /// The part of the file a write replaces that a chief is shown,
    /// where one is there and a chief is asked.
    replaced: Option<String>,
}

enum Unplanned {
    /// In words for the model.
    Failed(String),
    /// The file no longer holds what a chief was shown.
    Changed { now: Hashed },
}

impl Workspace {
    /// Carries out `request`, the change `pending` of `agent` that
    /// passed its checks, or answers why not.
    async fn change<R: Refusal>(
        &self,
        ctx: &Ctx<Self>,
        agent: AgentNumber,
        pending: &Pending,
        request: &Request<'_>,
    ) -> Result<ChangeAnswer<R>> {
        let action = request.asked.op();
        let path = text(request.path);
        let held = self.paths.hold(&path).await;
        let weighed = self
            .weighed(ctx, move || self.weigh(ctx, agent, request))
            .await?;
        let (passed, chief) = match weighed {
            Weighed::Answer(answer) => return Ok(answer),
            Weighed::Pass(passed) => (passed, None),
            Weighed::Chief(passed, needs, version) => {
                (passed, Some((needs, version)))
            }
        };
        let Passed {
            placement,
            sandbox,
            secrets,
        } = passed;
        let planned = match planned(
            placement.clone(),
            sandbox,
            &path,
            &request.asked,
            match chief {
                Some(_) => Planning::ForChief {
                    secrets: secrets.to_vec(),
                },
                None => Planning::Now,
            },
        )
        .await?
        {
            Ok(planned) => planned,
            Err(Unplanned::Failed(words)) => {
                return Ok(ChangeAnswer::Ended {
                    end: ChangeEnd::Failed {
                        words: scrub_text(&secrets, &words),
                    },
                });
            }
            Err(Unplanned::Changed { .. }) => {
                bail!("a change with nothing to match changed")
            }
        };
        if let Some((needs, reach_version)) = chief {
            let opening = pending::Opening {
                agent,
                op: action,
                request_hash: request.request_hash.clone(),
                needs,
                reach_version,
                base_hash: Some(hashed(planned.current.digest)),
                words: words_of(
                    &path,
                    &request.asked,
                    &planned,
                    &secrets,
                ),
                change: planned.diff,
                request: pending.clone(),
            };
            return self
                .wait_for_chief(ctx, request, &secrets, opening, held)
                .await;
        }
        let (temp_name, temp) = temp_of(&planned);
        let opened = transact_if(ctx, async |tx| {
            open(tx, agent, request, &temp).await
        })
        .await?;
        let id = match opened {
            Ok(id) => id,
            Err(answer) => return Ok(answer),
        };
        let end = self
            .carry_out(
                ctx,
                id,
                request,
                Carry {
                    placement,
                    sandbox,
                    secrets,
                    planned,
                    temp_name,
                },
                held,
            )
            .await?;
        Ok(ChangeAnswer::Ended { end })
    }

    /// Opens the wait of `opening`, a change of `request`, while its
    /// path is `held`, and tells the realm once the path is let go.
    async fn wait_for_chief<R: Refusal>(
        &self,
        ctx: &Ctx<Self>,
        request: &Request<'_>,
        secrets: &[String],
        opening: pending::Opening,
        held: OwnedMutexGuard<()>,
    ) -> Result<ChangeAnswer<R>> {
        let agent = opening.agent;
        let opened = transact_if(ctx, async |tx| {
            if let Some(call) =
                calls::find(tx, agent, request.call).await?
            {
                return Ok(Err(found(call, request)?));
            }
            Ok(pending::open(tx, secrets, opening).await?.map_err(
                |why| ChangeAnswer::Refused { why: why.into() },
            ))
        })
        .await?;
        let notice = match opened {
            Ok(notice) => notice,
            Err(answer) => return Ok(answer),
        };
        drop(held);
        let needs = notice.needs.clone();
        self.ask_chief(ctx, notice).await;
        Ok(ChangeAnswer::WaitsForChief { needs })
    }

    /// Carries out the change of `wait`, which a chief allowed, when
    /// the reach in force still lets it and the file still holds what
    /// the chief was shown. The agent is called with its answer.
    pub(in crate::workspace) async fn grant_change(
        &self,
        ctx: &Ctx<Self>,
        wait: &Wait,
    ) -> Result<AnswerTaken> {
        let request = Request::of(&wait.request)?;
        let path = text(request.path);
        let held = self.paths.hold(&path).await;
        let op = Op::Change {
            conversation: request.conversation,
            path: request.path,
        };
        let (reach, secrets) =
            match self.recheck(ctx, wait, &op).await? {
                Rechecked::Pass { reach, secrets, .. } => {
                    (reach, secrets)
                }
                Rechecked::Changed(changed) => {
                    drop(held);
                    let changed = pending::changed(
                        changed,
                        Refused::of_change::<FileGate>,
                    )?;
                    let why = NotRun::Change(changed.into());
                    return self.not_run(ctx, wait, why).await;
                }
            };
        let placement =
            self.placement(wait.agent, &reach, Purpose::FileTool)?;
        let was =
            wait.base_hash.context("a change waits with no hash")?;
        let planned = match planned(
            placement.clone(),
            self.sandbox,
            &path,
            &request.asked,
            Planning::Allowed { was },
        )
        .await?
        {
            Ok(planned) => planned,
            Err(Unplanned::Changed { now }) => {
                drop(held);
                let stale = Stale::FileChanged {
                    path: request.path.to_owned(),
                    was,
                    now,
                };
                let why = NotRun::Change(stale.into());
                return self.not_run(ctx, wait, why).await;
            }
            Err(Unplanned::Failed(words)) => {
                let end = ChangeEnd::Failed {
                    words: scrub_text(&secrets, &words),
                };
                let bytes = Some(calls::cbor(&end)?);
                if pending::settled(ctx, wait, "ended", bytes, None)
                    .await?
                    .is_none()
                {
                    return Ok(AnswerTaken::NoWait);
                }
                tell(
                    ctx,
                    wait.agent,
                    wait.call.clone(),
                    Outcome::Change { end },
                )
                .await;
                return Ok(AnswerTaken::Taken);
            }
        };
        let (temp_name, temp) = temp_of(&planned);
        let Some(id) = pending::settled(
            ctx,
            wait,
            "changing",
            None,
            Some(&temp),
        )
        .await?
        else {
            return Ok(AnswerTaken::NoWait);
        };
        let end = self
            .carry_out(
                ctx,
                id,
                &request,
                Carry {
                    placement,
                    sandbox: self.sandbox,
                    secrets,
                    planned,
                    temp_name,
                },
                held,
            )
            .await?;
        tell(
            ctx,
            wait.agent,
            wait.call.clone(),
            Outcome::Change { end },
        )
        .await;
        Ok(AnswerTaken::Taken)
    }

    /// Replaces the file of `request`, whose row `id` was written,
    /// as `carry` plans, records its end and lets go of the path.
    async fn carry_out(
        &self,
        ctx: &Ctx<Self>,
        id: i64,
        request: &Request<'_>,
        carry: Carry,
        held: OwnedMutexGuard<()>,
    ) -> Result<ChangeEnd> {
        let Carry {
            placement,
            sandbox,
            secrets,
            planned,
            temp_name,
        } = carry;
        // The guard goes with the replace, so the path stays held
        // until it is done even if this call is dropped meanwhile.
        let (replaced, held, planned) = {
            let path = text(request.path);
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
        let ended = scrubbed(
            &secrets,
            outcome(request.path, &request.asked, planned, replaced),
        );
        transact(ctx, async |tx| end(tx, id, &ended).await).await?;
        drop(held);
        Ok(ended)
    }

    /// Answers a repeat of `request` from its row, or passes it
    /// through the gate with `agent`'s reach.
    async fn weigh<R: Refusal>(
        &self,
        ctx: &Ctx<Self>,
        agent: AgentNumber,
        request: &Request<'_>,
    ) -> Result<Weighing<Weighed<R>>> {
        loop {
            let (found, in_force) = read_only(ctx, async |tx| {
                let found =
                    calls::find(tx, agent, request.call).await;
                let in_force = reach::in_force(tx, agent).await;
                Ok((found, in_force))
            })
            .await?;
            if let Some(call) = found? {
                match answered(call, request)? {
                    Row::Answer(answer) => {
                        return Ok(Weighing::Done(Weighed::Answer(
                            answer,
                        )));
                    }
                    // Also the row of a first copy that was dropped
                    // between its rename and its record: this copy
                    // then runs afresh on the file the first one
                    // changed, so an edit can answer that its old
                    // text was not found for a change that was made.
                    Row::Unfinished { id, temp } => {
                        if let Some(temp) = temp {
                            remove_new_file(&temp);
                        }
                        transact(ctx, async |tx| {
                            forget(tx, id).await
                        })
                        .await?;
                        continue;
                    }
                }
            }
            let in_force = in_force?;
            let op = Op::Change {
                conversation: request.conversation,
                path: request.path,
            };
            let reach = in_force.as_ref().map(|(_, reach)| reach);
            let gated = match self.gated(agent, reach, &op)? {
                Weighing::Done(gated) => gated,
                Weighing::NeedsSecrets(version) => {
                    return Ok(Weighing::NeedsSecrets(version));
                }
            };
            return Ok(Weighing::Done(match gated {
                Gated::Pass(passed) => Weighed::Pass(passed),
                Gated::Chief(passed, needs) => Weighed::Chief(
                    passed,
                    needs,
                    in_force
                        .map(|(version, _)| version)
                        .context("a chief is asked with no reach")?,
                ),
                Gated::Refused(refused) => {
                    Weighed::Answer(ChangeAnswer::Refused {
                        why: refused.of_change()?,
                    })
                }
            }));
        }
    }
}

struct Carry {
    placement: Placement,
    sandbox: hamlet_sandbox::Sandbox,
    secrets: Arc<Vec<String>>,
    planned: Planned,
    temp_name: String,
}

/// The name of the new file `planned` writes, and its path.
fn temp_of(planned: &Planned) -> (String, String) {
    let temp_name = format!(".hamlet-{}.new", uuid::Uuid::new_v4());
    let temp = path::join(&[
        path::dirname(&planned.current.real).as_str(),
        &temp_name,
    ]);
    (temp_name, temp)
}

enum Row<R> {
    Answer(ChangeAnswer<R>),
    Unfinished { id: i64, temp: Option<String> },
}

/// What a change's row `call` answers `request`, a call with its id.
/// A row of another request fails it with `message.id_reused`.
fn answered<R>(
    call: calls::Call,
    request: &Request<'_>,
) -> Result<Row<R>> {
    if !call.is(request.asked.op(), &request.request_hash) {
        return Err(id_reused(request.asked.action(), request.call));
    }
    Ok(match call.state.as_str() {
        "ended" => Row::Answer(ChangeAnswer::Ended {
            end: decoded("end", call.answer)?,
        }),
        "waiting" => Row::Answer(ChangeAnswer::WaitsForChief {
            needs: decoded("needs", call.answer)?,
        }),
        "refused" => Row::Answer(ChangeAnswer::Ended {
            end: ChangeEnd::NotRun {
                why: decoded("not_run", call.answer)?,
            },
        }),
        "changing" => Row::Unfinished {
            id: call.id,
            temp: call.temp,
        },
        state => bail!("a change has the state {state}"),
    })
}

/// What the row `call` answers `request` when another change with
/// its id is found while this one opens its own.
fn found<R>(
    call: calls::Call,
    request: &Request<'_>,
) -> Result<ChangeAnswer<R>> {
    Ok(match answered(call, request)? {
        Row::Answer(answer) => answer,
        Row::Unfinished { .. } => ChangeAnswer::Ended {
            end: ChangeEnd::Failed {
                words: "another change with this call id is at work"
                    .into(),
            },
        },
    })
}

/// The `what` a change's row holds in `bytes`.
fn decoded<T: serde::de::DeserializeOwned>(
    what: &str,
    bytes: Option<Vec<u8>>,
) -> Result<T> {
    let bytes = bytes
        .with_context(|| format!("a change's row has no {what}"))?;
    ciborium::from_reader(bytes.as_slice())
        .with_context(|| format!("decode a change's {what}"))
}

/// The change of `asked` at `path`, as `planned`, in words for a
/// chief, as the server's petition words it: each part with
/// `secrets` taken out, then cut.
fn words_of(
    path: &str,
    asked: &Asked<'_>,
    planned: &Planned,
    secrets: &[String],
) -> String {
    let part = |text: &str| cut(&scrub_text(secrets, text));
    let path = part(path);
    match (asked, &planned.replaced) {
        (Asked::Write(content), None) => {
            format!("{path}\nA new file:\n{}", part(content))
        }
        (Asked::Write(content), Some(old)) => format!(
            "{path}\nReplace:\n{old}\nWith:\n{}",
            part(content)
        ),
        (Asked::Edit(edits), _) => {
            edits.iter().fold(path, |words, edit| {
                format!(
                    "{words}\nReplace:\n{}\nWith:\n{}",
                    part(&edit.old),
                    part(&edit.new)
                )
            })
        }
    }
}

enum PlannedAs {
    Write(Vec<u8>),
    Edit(Vec<Replace>),
}

enum Planning {
    Now,
    ForChief { secrets: Vec<String> },
    Allowed { was: Hashed },
}

/// [`plan`], off the runtime.
async fn planned(
    placement: Placement,
    sandbox: hamlet_sandbox::Sandbox,
    path: &str,
    asked: &Asked<'_>,
    planning: Planning,
) -> Result<Result<Planned, Unplanned>> {
    let path = path.to_owned();
    let asked = match *asked {
        Asked::Write(content) => {
            PlannedAs::Write(content.as_bytes().to_vec())
        }
        Asked::Edit(edits) => PlannedAs::Edit(edits.to_vec()),
    };
    tokio::task::spawn_blocking(move || {
        plan(&placement, sandbox, &path, asked, &planning)
    })
    .await
    .context("a change's thread ended before it")
}

/// Reads what the change at `path` replaces, and works out what it
/// writes, or says in words why it cannot.
fn plan(
    placement: &Placement,
    sandbox: hamlet_sandbox::Sandbox,
    path: &str,
    asked: PlannedAs,
    planning: &Planning,
) -> Result<Planned, Unplanned> {
    let env = env(placement, sandbox);
    match asked {
        PlannedAs::Write(content) => {
            let mut replaced = match planning {
                Planning::ForChief { secrets } => {
                    Some(shown::Replaced::new(secrets))
                }
                Planning::Now | Planning::Allowed { .. } => None,
            };
            let current = env.and_then(|env| {
                env.read_for_change(path, &mut |part| {
                    if let Some(replaced) = &mut replaced {
                        replaced.push(part);
                    }
                })
            });
            let current = still(current, planning)?
                .map_err(|error| Unplanned::Failed(error.message))?;
            Ok(Planned {
                replaced: replaced
                    .filter(|_| current.digest.is_some())
                    .map(shown::Replaced::shown),
                current,
                content,
                diff: None,
            })
        }
        PlannedAs::Edit(edits) => {
            let mut bytes = Vec::new();
            let current = env.and_then(|env| {
                env.read_for_change(path, &mut |part| {
                    bytes.extend_from_slice(part);
                })
            });
            let current =
                still(current, planning)?.map_err(|error| {
                    Unplanned::Failed(
                        match error.code {
                            // A folder, or another file that is not
                            // regular.
                            Code::IsDirectory | Code::Invalid => {
                                CallError::NotAFile(path.to_owned())
                            }
                            code => CallError::Unedited {
                                path: path.to_owned(),
                                code,
                            },
                        }
                        .to_string(),
                    )
                })?;
            let (content, diff) = edit::edited(
                path,
                current.digest.is_some().then_some(bytes),
                &edits,
            )
            .map_err(|error| Unplanned::Failed(error.to_string()))?;
            Ok(Planned {
                current,
                content: content.into_bytes(),
                diff: Some(diff),
                replaced: None,
            })
        }
    }
}

/// `current`, unless a chief allowed the change and the file it read
/// no longer hashes to what the chief was shown.
fn still(
    current: Result<Current, FileError>,
    planning: &Planning,
) -> Result<Result<Current, FileError>, Unplanned> {
    if let (Planning::Allowed { was }, Ok(current)) =
        (planning, &current)
    {
        let now = hashed(current.digest);
        if now != *was {
            return Err(Unplanned::Changed { now });
        }
    }
    Ok(current)
}

fn hashed(digest: Option<[u8; 32]>) -> Hashed {
    digest.map_or(Hashed::Absent, |digest| Hashed::Sha256 {
        hash: bytes::Sha256(digest),
    })
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
        planned.current.digest.as_ref(),
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
async fn open<R>(
    tx: &SqliteTransaction,
    agent: AgentNumber,
    request: &Request<'_>,
    temp: &str,
) -> Result<Result<i64, ChangeAnswer<R>>> {
    if let Some(call) = calls::find(tx, agent, request.call).await? {
        return Ok(Err(found(call, request)?));
    }
    calls::insert(
        tx,
        agent,
        request.call,
        request.asked.op(),
        request.request_hash.clone(),
        "changing",
        None,
        Some(temp),
    )
    .await
    .map(Ok)
}

/// Records the end of the change `id`, in `tx`.
async fn end(
    tx: &SqliteTransaction,
    id: i64,
    ended: &ChangeEnd,
) -> Result<()> {
    let bytes = calls::cbor(ended)?;
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

/// The end of a change of `path` that planned `planned` and was
/// `replaced`.
fn outcome(
    path: &Path,
    asked: &Asked<'_>,
    planned: Planned,
    replaced: Result<Replaced, FileError>,
) -> ChangeEnd {
    match replaced {
        Ok(Replaced::Done { created }) => {
            ChangeEnd::Changed(Change {
                path: path.to_owned(),
                created,
                hash: bytes::Sha256(
                    Sha256::digest(&planned.content).into(),
                ),
                diff: planned.diff,
            })
        }
        Ok(Replaced::Changed(now)) => ChangeEnd::NotRun {
            why: Stale::FileChanged {
                path: path.to_owned(),
                was: hashed(planned.current.digest),
                now: hashed(now),
            }
            .into(),
        },
        Err(error) => ChangeEnd::Failed {
            words: match asked {
                Asked::Write(_) => error.message,
                Asked::Edit(_) => CallError::Unedited {
                    path: text(path),
                    code: error.code,
                }
                .to_string(),
            },
        },
    }
}

/// `ended` with `secrets` taken out of every word (C32).
fn scrubbed(secrets: &[String], ended: ChangeEnd) -> ChangeEnd {
    let scrub = |words: &str| scrub_text(secrets, words);
    match ended {
        ChangeEnd::Changed(mut change) => {
            if let Some(diff) = &mut change.diff {
                diff.diff = scrub(&diff.diff);
                diff.patch = scrub(&diff.patch);
            }
            ChangeEnd::Changed(change)
        }
        ChangeEnd::Failed { words } => ChangeEnd::Failed {
            words: scrub(&words),
        },
        not_run @ ChangeEnd::NotRun { .. } => not_run,
    }
}

#[cfg(test)]
mod tests {
    use hamlet_actor_error::ActorError;

    use super::*;

    fn planned(was: Option<&[u8]>) -> Planned {
        Planned {
            current: Current {
                real: "/p/a.md".into(),
                digest: was.map(|was| Sha256::digest(was).into()),
            },
            content: b"new".to_vec(),
            diff: None,
            replaced: None,
        }
    }

    /// A change's row answers a repeat of it once it ended. One that
    /// did not end, as a failure after its row leaves it, is cut off
    /// so the repeat runs afresh; a row of another request fails it
    /// with `message.id_reused` (M-E10).
    #[test]
    fn a_row_answers_a_repeat_or_is_cut_off() {
        let call = CallId("w1".into());
        let conversation = ConversationId("c1".into());
        let request = Request {
            call: &call,
            conversation: &conversation,
            path: Path::new("/p/a.md"),
            asked: Asked::Write("new"),
            request_hash: vec![1],
        };
        let end = ChangeEnd::Failed { words: "no".into() };
        let bytes = calls::cbor(&end).unwrap();
        let row =
            |op: &str, hash: u8, state: &str, answer| calls::Call {
                id: 4,
                op: op.into(),
                request_hash: vec![hash],
                state: state.into(),
                answer,
                temp: Some("/p/.hamlet-x.new".into()),
            };

        let answered =
            |call| answered::<ChangeRefusal>(call, &request);
        let ended = answered(row("write", 1, "ended", Some(bytes)));
        let unfinished = answered(row("write", 1, "changing", None));
        let other = answered(row("write", 2, "changing", None));
        let a_run = answered(row("run", 1, "running", None));

        assert!(matches!(
            ended.unwrap(),
            Row::Answer(ChangeAnswer::Ended { end: given }) if given == end
        ));
        assert!(matches!(
            unfinished.unwrap(),
            Row::Unfinished { id: 4, temp: Some(temp) }
                if temp == "/p/.hamlet-x.new"
        ));
        for reused in [other, a_run] {
            let Err(error) = reused else {
                panic!("a reused id was answered");
            };
            let error = ActorError::extract(&error);
            assert_eq!(
                (error.group(), error.code()),
                ("message", "id_reused")
            );
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
            current: env.read_for_change(&file, &mut |_| ()).unwrap(),
            content: b"new".to_vec(),
            diff: None,
            replaced: None,
        };
        std::fs::write(&file, "theirs").unwrap();

        let replaced = carried_out(&env, &file, ".a.new", &planned);

        assert_eq!(
            replaced.unwrap(),
            Replaced::Changed(Some(Sha256::digest(b"theirs").into()))
        );
        assert_eq!(std::fs::read(&file).unwrap(), b"theirs");
    }

    /// A file that no longer held what the change read is not
    /// changed, with what it held and what it holds.
    #[test]
    fn a_file_that_changed_is_not_changed_with_both_hashes() {
        let refused = outcome(
            Path::new("/p/a.md"),
            &Asked::Write("new"),
            planned(Some(b"old")),
            Ok(Replaced::Changed(Some(
                Sha256::digest(b"theirs").into(),
            ))),
        );
        let gone = outcome(
            Path::new("/p/a.md"),
            &Asked::Write("new"),
            planned(Some(b"old")),
            Ok(Replaced::Changed(None)),
        );

        let old = Hashed::Sha256 {
            hash: bytes::Sha256(Sha256::digest(b"old").into()),
        };
        assert_eq!(
            refused,
            ChangeEnd::NotRun {
                why: Stale::FileChanged {
                    path: "/p/a.md".into(),
                    was: old,
                    now: Hashed::Sha256 {
                        hash: bytes::Sha256(
                            Sha256::digest(b"theirs").into()
                        ),
                    },
                }
                .into()
            }
        );
        assert_eq!(
            gone,
            ChangeEnd::NotRun {
                why: Stale::FileChanged {
                    path: "/p/a.md".into(),
                    was: old,
                    now: Hashed::Absent,
                }
                .into()
            }
        );
    }

    /// A replace that failed answers a write in the file's words, and
    /// an edit in the words of the server's tools for an edit.
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
            ChangeEnd::Failed {
                words: "EACCES: permission denied, open '/p/a.md'"
                    .into()
            }
        );
        assert_eq!(
            edited,
            ChangeEnd::Failed {
                words: "Could not edit file: /p/a.md. Error code: \
                        permission_denied."
                    .into()
            }
        );
    }
}
