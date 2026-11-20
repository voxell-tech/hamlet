//! Parcels, the files a letter or a question sends: the `parcel` and
//! `parcel_file` tables, their steps, `FetchPart`, and the store the
//! realm keeps their files in.
//!
//! The store's rule: a file is named by its hash and written whole
//! under a temporary name before its rename, so a kill leaves at most
//! a temporary file; a parcel's steps driven again at the realm's
//! start copy what is missing; at the start and after each parcel's
//! steps, the realm removes the files no unswept parcel names and the
//! temporary files no copy is writing. No file changes after its
//! rename.

use std::collections::HashSet;
use std::io::SeekFrom;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context as _, Result, bail};
use hamlet_actor::{BindParam, Ctx, Handler};
use hamlet_host::HostRef;
use hamlet_messages::address::{self, Agent, Party as _, Workspace};
use hamlet_messages::agent::{Input, InputId, Submit};
use hamlet_messages::bytes::{Base64, Sha256};
use hamlet_messages::realm::client::Answer;
use hamlet_messages::realm::parcels::{
    FetchPart, Measure, PARCEL_PART, ParcelFile, ParcelId, Part,
    PartRefusal, part_start,
};
use hamlet_messages::realm::world::WaitId;
use hamlet_messages::realm::{Enclosed, LetterId};
use hamlet_messages::sender::sender;
use hamlet_messages::workspace::{
    self, AgentNumber, ConversationId, ParcelAnswer, ParcelEntry,
    ParcelFault, ParcelPart, ParcelRefusal,
};
use hamlet_protocol::client::{AttachedFile, Choice};
use serde::{Deserialize, Serialize};
use sha2::Digest as _;
use tokio::io::{
    AsyncReadExt as _, AsyncSeekExt as _, AsyncWriteExt as _,
};

use super::calls::{self, Calls, Delivery};
use super::change::{self, Affected, Rows, Writer, number};
use super::letters::Kind;
use super::oversight::asking;
use super::peer::When;
use super::steps::{self, Cx, StepId, Steps};
use super::tasks::TaskId;
use super::waits::{self, Raised, Ruling};
use super::{Realm, record, tree};

/// How long a delivered parcel keeps its files: its `at` is when it
/// was sent until it is ready, then when it became ready, or when a
/// chief let its held letter go.
const KEPT_MS: i64 = 24 * 60 * 60 * 1000;

/// The bit of a step row's number that marks the steps a chief's
/// allow begins: a parcel's first steps are row `parcel`, and those
/// after an allow are row `parcel | AFTER_ALLOW`.
const AFTER_ALLOW: u64 = 1 << 32;

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
enum State {
    Measuring,
    /// Over the cap, it waits for a chief.
    Waiting,
    Copying,
    Ready,
    Refused,
}

#[derive(Debug, PartialEq)]
pub(super) enum Change {
    /// The parcel of `letter`, kept in the same commit, with the
    /// paths it names.
    Open {
        parcel: ParcelId,
        letter: LetterId,
        paths: Vec<PathBuf>,
    },
    /// Its files, measured, and whether it waits for a chief.
    Measured {
        parcel: ParcelId,
        files: Vec<ParcelEntry>,
        waits: bool,
    },
    /// A chief allowed it.
    Allowed {
        parcel: ParcelId,
    },
    Ready {
        parcel: ParcelId,
    },
    Refused {
        parcel: ParcelId,
    },
    /// The parcels to `receiver` that wait for a chief are refused:
    /// it has left.
    ReceiverLeft {
        receiver: AgentNumber,
    },
    /// Their files may go.
    Swept {
        parcels: Vec<ParcelId>,
    },
    /// The letters held on `task` are given now, so the day their
    /// parcels keep their files begins.
    Released {
        task: TaskId,
    },
}

pub(super) async fn apply(
    writer: &Writer<'_>,
    change: Change,
) -> Result<Affected> {
    match change {
        Change::Open {
            parcel,
            letter,
            paths,
        } => {
            let opened = writer
                .execute(
                    "INSERT INTO parcel (id, letter, sender, \
                     receiver, paths, state, at) SELECT ?1, id, \
                     sender, receiver, ?2, ?3, ?4 FROM letter WHERE \
                     id = ?5",
                    vec![
                        id(parcel),
                        BindParam::Text(serde_json::to_string(
                            &paths,
                        )?),
                        state(State::Measuring)?,
                        BindParam::Integer(writer.at().0),
                        change::integer(letter.0)?,
                    ],
                )
                .await?;
            if opened == 0 {
                bail!(
                    "parcel {} has no letter {}",
                    parcel.0,
                    letter.0
                );
            }
        }
        Change::Measured {
            parcel,
            files,
            waits,
        } => {
            for (file, entry) in files.into_iter().enumerate() {
                writer
                    .execute(
                        "INSERT INTO parcel_file (parcel, file, \
                         path, name, size, sha256) VALUES (?1, ?2, \
                         ?3, ?4, ?5, ?6)",
                        vec![
                            id(parcel),
                            change::integer(file)?,
                            BindParam::Text(path_text(&entry.path)?),
                            BindParam::Text(path_text(&entry.name)?),
                            change::integer(entry.size)?,
                            BindParam::Blob(entry.sha256.0.to_vec()),
                        ],
                    )
                    .await?;
            }
            let then = if waits {
                State::Waiting
            } else {
                State::Copying
            };
            moved(writer, parcel, &[State::Measuring], then).await?;
        }
        Change::Allowed { parcel } => {
            moved(writer, parcel, &[State::Waiting], State::Copying)
                .await?;
        }
        Change::Ready { parcel } => {
            writer
                .execute(
                    "UPDATE parcel SET state = ?1, at = ?2 WHERE id \
                     = ?3 AND state = ?4",
                    vec![
                        state(State::Ready)?,
                        BindParam::Integer(writer.at().0),
                        id(parcel),
                        state(State::Copying)?,
                    ],
                )
                .await?;
        }
        Change::Refused { parcel } => {
            let unrefused = [
                State::Measuring,
                State::Waiting,
                State::Copying,
                State::Ready,
            ];
            moved(writer, parcel, &unrefused, State::Refused).await?;
        }
        Change::ReceiverLeft { receiver } => {
            writer
                .execute(
                    "UPDATE parcel SET state = ?1 WHERE receiver = \
                     ?2 AND state = ?3",
                    vec![
                        state(State::Refused)?,
                        number(receiver),
                        state(State::Waiting)?,
                    ],
                )
                .await?;
        }
        Change::Released { task } => {
            writer
                .execute(
                    "UPDATE parcel SET at = ?1 WHERE state = ?2 AND \
                     letter IN (SELECT letter FROM held WHERE task \
                     = ?3)",
                    vec![
                        BindParam::Integer(writer.at().0),
                        state(State::Ready)?,
                        task.column()?,
                    ],
                )
                .await?;
        }
        Change::Swept { parcels } => {
            for parcel in parcels {
                writer
                    .execute(
                        "UPDATE parcel SET swept_at = ?1 WHERE id = \
                         ?2 AND swept_at IS NULL",
                        vec![
                            BindParam::Integer(writer.at().0),
                            id(parcel),
                        ],
                    )
                    .await?;
            }
        }
    }
    Ok(Affected::default())
}

async fn moved(
    writer: &Writer<'_>,
    parcel: ParcelId,
    from: &[State],
    to: State,
) -> Result<()> {
    for &from in from {
        writer
            .execute(
                "UPDATE parcel SET state = ?1 WHERE id = ?2 AND \
                 state = ?3",
                vec![state(to)?, id(parcel), state(from)?],
            )
            .await?;
    }
    Ok(())
}

fn id(parcel: ParcelId) -> BindParam {
    BindParam::Integer(i64::from(parcel.0))
}

fn state(state: State) -> Result<BindParam> {
    Ok(BindParam::Text(change::case_name(&state)?))
}

fn path_text(path: &std::path::Path) -> Result<String> {
    path.to_str()
        .map(str::to_owned)
        .with_context(|| format!("{} is not UTF-8", path.display()))
}

/// The changes that send the files under `paths` with `letter`: its
/// parcel and the parcel's steps, which deliver the letter when the
/// files are copied.
pub(super) async fn start(
    rows: &Rows<'_>,
    letter: LetterId,
    paths: &[Enclosed],
) -> Result<Option<Vec<change::Change>>> {
    let parcel =
        ParcelId(u32::try_from(rows.next_id("parcel").await?)?);
    Ok(Some(vec![
        Change::Open {
            parcel,
            letter,
            paths: paths
                .iter()
                .map(|path| path.path.clone())
                .collect(),
        }
        .into(),
        steps::Change::Begin {
            row: row(parcel, false),
        }
        .into(),
    ]))
}

fn row(parcel: ParcelId, after_allow: bool) -> StepId {
    let mark = if after_allow { AFTER_ALLOW } else { 0 };
    StepId {
        kind: steps::Kind::Parcel,
        n: u64::from(parcel.0) | mark,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    /// The workspace measured the files.
    Measure,
    /// The files are copied into the realm.
    Copy,
    /// The letter is delivered with its files.
    Deliver,
}

impl Steps for Step {
    const ALL: &'static [Self] =
        &[Self::Measure, Self::Copy, Self::Deliver];

    fn name(self) -> &'static str {
        match self {
            Self::Measure => "measure",
            Self::Copy => "copy",
            Self::Deliver => "deliver",
        }
    }
}

/// A parcel as its steps read it.
struct Sent {
    parcel: ParcelId,
    letter: LetterId,
    sender: AgentNumber,
    receiver: AgentNumber,
    paths: Vec<PathBuf>,
    state: State,
    kind: Kind,
    text: String,
    task: Option<TaskId>,
    conversation: Option<ConversationId>,
    files: Vec<ParcelEntry>,
}

impl Sent {
    async fn read(rows: &Rows<'_>, parcel: ParcelId) -> Result<Self> {
        let (letter, sender, receiver, paths, state) = rows
            .row::<(u32, u32, u32, String, String)>(
                "SELECT letter, sender, receiver, paths, state FROM \
                 parcel WHERE id = ?1",
                vec![id(parcel)],
            )
            .await?
            .with_context(|| format!("no parcel {}", parcel.0))?;
        let (kind, text, task, conversation) = rows
            .row::<(String, String, Option<TaskId>, Option<String>)>(
                "SELECT kind, text, task, conversation FROM letter \
                 WHERE id = ?1",
                vec![change::integer(letter)?],
            )
            .await?
            .with_context(|| format!("no letter {letter}"))?;
        Ok(Self {
            parcel,
            letter: LetterId(letter),
            sender: AgentNumber(sender),
            receiver: AgentNumber(receiver),
            paths: serde_json::from_str(&paths)?,
            state: change::from_case_name(state)?,
            kind: change::from_case_name(kind)?,
            text,
            task,
            conversation: conversation.map(ConversationId),
            files: entries(rows, parcel).await?,
        })
    }

    /// Whether this row's steps copy and deliver it: those a chief's
    /// allow begins when it waited for one, else the first row's.
    fn sends(&self, after_allow: bool) -> bool {
        waits(&self.files) == after_allow
    }

    /// Refuses the parcel, and tells the sender why in its turn.
    async fn refused(
        &self,
        cx: &Cx<'_>,
        why: &ParcelRefusal,
    ) -> Result<Vec<change::Change>> {
        let refused = Change::Refused {
            parcel: self.parcel,
        };
        cx.commit_delivering(
            change::Transaction::begin(cx.ctx).await?,
            vec![refused.into()],
            self.notice(told(why)),
        )
        .await?;
        Ok(Vec::new())
    }

    fn notice(&self, text: String) -> Calls {
        Calls::after_answer(Delivery::ParcelRefused {
            to: self.sender,
            submit: Submit {
                id: InputId(format!("parcel/{}", self.parcel.0)),
                task: self.task.map(TaskId::on_the_wire),
                input: Input::Notice {
                    text,
                    conversation: self.conversation.clone(),
                },
            },
        })
    }
}

/// The words the sender reads when its parcel fails for good.
const UNSENT: &str = "The files could not be sent.";

/// The words the sender reads for `why`, as today's tools gave them:
/// a file that cannot be read, or a workspace that cannot be opened,
/// in the words that say so, and any other refusal as a sentence.
fn told(why: &ParcelRefusal) -> String {
    let words = why.to_string();
    match why {
        ParcelRefusal::Parcel(
            ParcelFault::Unreadable { .. }
            | ParcelFault::Unopened { .. },
        ) => words,
        _ => sentence(&words),
    }
}

fn sentence(words: &str) -> String {
    let mut chars = words.chars();
    let first =
        chars.next().map(|c| c.to_uppercase().collect::<String>());
    format!("{}{}.", first.unwrap_or_default(), chars.as_str())
}

async fn entries(
    rows: &Rows<'_>,
    parcel: ParcelId,
) -> Result<Vec<ParcelEntry>> {
    rows.rows::<(String, String, u64, Vec<u8>)>(
        "SELECT path, name, size, sha256 FROM parcel_file WHERE \
         parcel = ?1 ORDER BY file",
        vec![id(parcel)],
    )
    .await?
    .into_iter()
    .map(|(path, name, size, sha256)| {
        Ok(ParcelEntry {
            path: path.into(),
            name: name.into(),
            size,
            sha256: hash_of(sha256)?,
        })
    })
    .collect()
}

/// A `sha256` column's hash.
fn hash_of(column: Vec<u8>) -> Result<Sha256> {
    let bytes = <[u8; 32]>::try_from(column).map_err(|column| {
        anyhow::anyhow!("a hash of {} bytes", column.len())
    })?;
    Ok(Sha256(bytes))
}

fn measure(files: &[ParcelEntry]) -> Measure {
    Measure {
        files: u32::try_from(files.len()).unwrap_or(u32::MAX),
        bytes: files.iter().map(|file| file.size).sum(),
    }
}

/// Whether `files` are more than a chief lets go unasked.
fn waits(files: &[ParcelEntry]) -> bool {
    let Measure { files, bytes } = measure(files);
    bytes > AttachedFile::CAP_BYTES
        || usize::try_from(files).unwrap_or(usize::MAX)
            > AttachedFile::CAP_FILES
}

/// Does `step` of the parcel's row `n`, and returns the changes
/// committed with its progress.
pub(super) async fn run(
    cx: &Cx<'_>,
    n: u64,
    step: Step,
) -> Result<Vec<change::Change>> {
    let parcel = parcel_of(n)?;
    let after_allow = n & AFTER_ALLOW != 0;
    let rows = cx.rows()?;
    let sent = Sent::read(&rows, parcel).await?;
    let open = matches!(
        sent.state,
        State::Measuring | State::Waiting | State::Copying
    );
    if open && either_left(&rows, sent.sender, sent.receiver).await? {
        return Ok(vec![Change::Refused { parcel }.into()]);
    }
    match step {
        Step::Measure if sent.state == State::Measuring => {
            measured(cx, &sent).await
        }
        Step::Copy
            if sent.state == State::Copying
                && sent.sends(after_allow) =>
        {
            copy(cx, &sent).await
        }
        Step::Deliver => {
            if sent.sends(after_allow)
                && matches!(sent.state, State::Copying | State::Ready)
            {
                deliver(cx, &sent).await?;
            }
            sweep(cx.realm, cx.ctx).await?;
            Ok(Vec::new())
        }
        Step::Measure | Step::Copy => Ok(Vec::new()),
    }
}

/// The changes that refuse the parcel of row `n`, whose step failed
/// for good, and the notice that tells its sender, unless it was
/// refused already or its sender or receiver has left.
pub(super) async fn failed(
    rows: &Rows<'_>,
    n: u64,
) -> Result<(Vec<change::Change>, Calls)> {
    let parcel = parcel_of(n)?;
    let sent = Sent::read(rows, parcel).await?;
    let changes = vec![
        Change::Refused { parcel }.into(),
        steps::Change::Close {
            row: StepId {
                kind: steps::Kind::Parcel,
                n,
            },
        }
        .into(),
    ];
    let told = sent.state != State::Refused
        && !either_left(rows, sent.sender, sent.receiver).await?;
    let calls = if told {
        sent.notice(UNSENT.to_owned())
    } else {
        Calls::none()
    };
    Ok((changes, calls))
}

fn parcel_of(n: u64) -> Result<ParcelId> {
    Ok(ParcelId(u32::try_from(n & (AFTER_ALLOW - 1))?))
}

/// Whether `sender` or `receiver` has ended, so that no one is told
/// of their parcel.
async fn either_left(
    rows: &Rows<'_>,
    sender: AgentNumber,
    receiver: AgentNumber,
) -> Result<bool> {
    Ok(tree::left(rows, sender).await?.is_some()
        || tree::left(rows, receiver).await?.is_some())
}

/// What the parcel's measure makes of it: refused, waiting for a
/// chief, or to be copied.
async fn measured(
    cx: &Cx<'_>,
    sent: &Sent,
) -> Result<Vec<change::Change>> {
    let asked = workspace::Parcel {
        agent: sent.sender,
        paths: sent
            .paths
            .iter()
            .map(|path| Enclosed { path: path.clone() })
            .collect(),
    };
    let files = match cx
        .peers()
        .call(&Workspace, &asked, When::AfterAnswer)
        .await?
    {
        ParcelAnswer::Measured { files } => files,
        ParcelAnswer::Refused { why } => {
            return sent.refused(cx, &why).await;
        }
    };
    let parcel = sent.parcel;
    if !waits(&files) {
        return Ok(vec![
            Change::Measured {
                parcel,
                files,
                waits: false,
            }
            .into(),
        ]);
    }
    let measure = measure(&files);
    let mut detail = Vec::new();
    ciborium::into_writer(&Sending { parcel, measure }, &mut detail)?;
    let raised = Raised {
        agent: sent.sender,
        kind: waits::Kind::Send,
        task: sent.task,
        conversation: sent.conversation.clone(),
        subject: subject(&cx.rows()?, sent, measure).await?,
        detail,
    };
    let tx = change::Transaction::begin(cx.ctx).await?;
    let wait = waits::next(&tx.rows()).await?;
    let pushes = tx
        .commit(
            cx.realm,
            change::Write::<()> {
                changes: vec![
                    Change::Measured {
                        parcel,
                        files,
                        waits: true,
                    }
                    .into(),
                    waits::Change::Raise { wait, raised }.into(),
                ],
                entries: Vec::new(),
                marker: None,
            },
        )
        .await?;
    calls::push_all(cx.realm, cx.ctx, pushes).await;
    Ok(Vec::new())
}

/// A send wait's detail.
#[derive(Serialize, Deserialize)]
struct Sending {
    parcel: ParcelId,
    measure: Measure,
}

/// The measure a send wait's `detail` keeps.
pub(super) fn measure_of(detail: &[u8]) -> Result<Measure> {
    Ok(ciborium::from_reader::<Sending, _>(detail)?.measure)
}

/// The words a chief reads on a parcel's wait, as today's gate gave
/// them: the size and the files, then the call.
async fn subject(
    rows: &Rows<'_>,
    sent: &Sent,
    Measure { files, bytes }: Measure,
) -> Result<String> {
    let (tool, said) = match sent.kind {
        Kind::Letter => ("tell", "text"),
        Kind::Question => ("ask", "question"),
    };
    let args = serde_json::json!({
        "to": record::address(rows, sent.receiver).await?,
        said: sent.text,
        "paths": sent.paths,
    });
    Ok(format!(
        "{} in {files} files: {tool} {args}",
        AttachedFile::size_words(bytes)
    ))
}

/// Copies each file the store lacks from the sender's workspace,
/// part by part, checking its hash before its rename.
async fn copy(
    cx: &Cx<'_>,
    sent: &Sent,
) -> Result<Vec<change::Change>> {
    let store = Store::of(cx.ctx)?;
    tokio::fs::create_dir_all(&store.0).await?;
    for (file, entry) in sent.files.iter().enumerate() {
        let kept = store.file(&entry.sha256);
        if tokio::fs::try_exists(&kept).await? {
            continue;
        }
        let temporary = store.temporary(sent.parcel, file);
        let copied =
            copy_file(cx, sent.sender, entry, &temporary).await;
        if !matches!(copied, Ok(None)) {
            let _ = tokio::fs::remove_file(&temporary).await;
        }
        if let Some(why) = copied? {
            return sent.refused(cx, &why).await;
        }
        tokio::fs::rename(&temporary, &kept).await?;
    }
    Ok(Vec::new())
}

/// Writes `entry` whole to `temporary`, or says why it cannot.
async fn copy_file(
    cx: &Cx<'_>,
    sender: AgentNumber,
    entry: &ParcelEntry,
    temporary: &std::path::Path,
) -> Result<Option<ParcelRefusal>> {
    let mut out = tokio::fs::File::create(temporary).await?;
    let mut hash = sha2::Sha256::new();
    let mut written = 0u64;
    let mut part = 0;
    loop {
        let asked = ParcelPart {
            agent: sender,
            path: entry.path.clone(),
            part,
        };
        let (bytes, last) = match cx
            .peers()
            .call(&Workspace, &asked, When::AfterAnswer)
            .await?
        {
            Part::Bytes { bytes, last } => (bytes.0, last),
            Part::Refused {
                why:
                    ParcelRefusal::Parcel(ParcelFault::NoSuchPart {
                        ..
                    }),
            } => return Ok(Some(changed(entry))),
            Part::Refused { why } => return Ok(Some(why)),
        };
        written += bytes.len() as u64;
        if written > entry.size {
            break;
        }
        hash = hash.chain_update(&bytes);
        out.write_all(&bytes).await?;
        if last {
            break;
        }
        part += 1;
    }
    out.sync_all().await?;
    if written != entry.size
        || Sha256(hash.finalize().into()) != entry.sha256
    {
        return Ok(Some(changed(entry)));
    }
    Ok(None)
}

/// Why `entry` is not sent when it grew or shrank, or its bytes
/// changed, after its measure.
fn changed(entry: &ParcelEntry) -> ParcelRefusal {
    ParcelFault::Unreadable {
        path: entry.name.clone(),
        words: "it changed while it was sent.".to_owned(),
    }
    .into()
}

/// Marks the parcel ready and delivers its letter, or keeps it while
/// its task is held. A drive again after a kill delivers it again,
/// under the same input's id, or keeps it if its task is held by
/// then.
async fn deliver(cx: &Cx<'_>, sent: &Sent) -> Result<()> {
    let letter = || Delivery::Letter(sent.letter);
    let tx = change::Transaction::begin(cx.ctx).await?;
    let holding = asking::holds(
        &tx.rows(),
        sent.sender,
        sent.receiver,
        sent.task,
        letter(),
    )
    .await?;
    let delivers = holding.is_none();
    let mut changes = Vec::new();
    if sent.state == State::Copying {
        changes.push(
            Change::Ready {
                parcel: sent.parcel,
            }
            .into(),
        );
    }
    if let Some(asking::Holding::Held(held)) = holding {
        changes.push(held.into());
    }
    let deliveries = if delivers {
        Calls::after_answer(letter())
    } else {
        Calls::none()
    };
    cx.commit_delivering(tx, changes, deliveries).await?.taken()
}

impl Handler<FetchPart> for Realm {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        fetch: FetchPart,
    ) -> Result<Part<PartRefusal>> {
        let Agent(by) = sender(&ctx, &fetch)?;
        let rows = Rows::outside(&ctx)?;
        let Some((receiver, letter, state, swept)) = rows
            .row::<(u32, u32, String, Option<i64>)>(
                "SELECT receiver, letter, state, swept_at FROM \
                 parcel WHERE id = ?1",
                vec![id(fetch.parcel)],
            )
            .await?
        else {
            return Ok(Part::Refused {
                why: PartRefusal::NoSuchParcel,
            });
        };
        if AgentNumber(receiver) != by {
            return Ok(Part::Refused {
                why: PartRefusal::NotYours,
            });
        }
        if change::from_case_name::<State>(state)? != State::Ready
            || swept.is_some()
            || tree::left(&rows, by).await?.is_some()
            || asking::still_held(&rows, LetterId(letter)).await?
        {
            return Ok(Part::Refused {
                why: PartRefusal::NoSuchParcel,
            });
        }
        let file = rows
            .row::<(u64, Vec<u8>)>(
                "SELECT size, sha256 FROM parcel_file WHERE parcel \
                 = ?1 AND file = ?2",
                vec![id(fetch.parcel), change::integer(fetch.file)?],
            )
            .await?;
        let Some((size, sha256)) = file else {
            return Ok(Part::Refused {
                why: PartRefusal::NoSuchPart,
            });
        };
        let Some(start) = part_start(size, fetch.part) else {
            return Ok(Part::Refused {
                why: PartRefusal::NoSuchPart,
            });
        };
        let kept = Store::of(&ctx)?.file(&hash_of(sha256)?);
        let mut file = tokio::fs::File::open(&kept).await?;
        file.seek(SeekFrom::Start(start)).await?;
        let mut bytes = Vec::new();
        file.take(PARCEL_PART as u64)
            .read_to_end(&mut bytes)
            .await?;
        let last = start + bytes.len() as u64 >= size;
        Ok(Part::Bytes {
            bytes: Base64(bytes),
            last,
        })
    }
}

/// The files of `letter`'s parcel, as its receiver is told of them:
/// none until they are copied, or once they are swept.
pub(super) async fn files(
    rows: &Rows<'_>,
    letter: LetterId,
) -> Result<Vec<ParcelFile>> {
    let Some((parcel, sender)) = rows
        .row::<(u32, u32)>(
            "SELECT id, sender FROM parcel WHERE letter = ?1 AND \
             state = ?2 AND swept_at IS NULL",
            vec![change::integer(letter.0)?, state(State::Ready)?],
        )
        .await?
    else {
        return Ok(Vec::new());
    };
    let from = record::address(rows, AgentNumber(sender))
        .await?
        .replace('#', "-");
    let parcel = ParcelId(parcel);
    entries(rows, parcel)
        .await?
        .into_iter()
        .enumerate()
        .map(|(file, entry)| {
            Ok(ParcelFile {
                parcel,
                file: u32::try_from(file)?,
                path: PathBuf::from(format!("from-{from}"))
                    .join(entry.name),
                size: entry.size,
            })
        })
        .collect()
}

/// Removes the realm's files no unswept parcel names, and the
/// temporary files no copy is writing. First it ends the parcels done
/// with, as [`done`] says.
pub(super) async fn sweep(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
) -> Result<()> {
    let changes = done(&Rows::outside(ctx)?).await?;
    if !changes.is_empty() {
        change::commit(realm, ctx, changes).await?;
    }
    let store = Store::of(ctx)?;
    let listed = match tokio::fs::read_dir(&store.0).await {
        Ok(listed) => listed,
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound =>
        {
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    };
    let names = names_in(listed).await?;
    let tx = change::Transaction::begin(ctx).await?;
    let rows = tx.rows();
    let named = rows
        .values::<Vec<u8>>(
            "SELECT DISTINCT parcel_file.sha256 FROM parcel_file \
             JOIN parcel ON parcel.id = parcel_file.parcel WHERE \
             parcel.swept_at IS NULL",
            Vec::new(),
        )
        .await?
        .into_iter()
        .map(|sha256| Ok(hash_of(sha256)?.to_string()))
        .collect::<Result<HashSet<_>>>()?;
    let copying = rows
        .values::<u32>(
            "SELECT id FROM parcel WHERE state = ?1",
            vec![state(State::Copying)?],
        )
        .await?
        .into_iter()
        .collect::<HashSet<_>>();
    for name in names {
        let stays = match Store::kept_as(&name) {
            Kept::File => named.contains(&name),
            Kept::Temporary(parcel) => copying.contains(&parcel),
            Kept::Other => true,
        };
        if !stays {
            match tokio::fs::remove_file(store.0.join(&name)).await {
                Err(error)
                    if error.kind()
                        != std::io::ErrorKind::NotFound =>
                {
                    return Err(error.into());
                }
                _ => {}
            }
        }
    }
    Ok(())
}

/// The changes that end the parcels done with: one waiting for a
/// chief while its sender or receiver has ended is refused, and one
/// refused, or delivered a day ago and not held, is swept.
async fn done(rows: &Rows<'_>) -> Result<Vec<change::Change>> {
    let unswept = rows
        .rows::<(u32, u32, u32, u32, String, i64)>(
            "SELECT id, letter, sender, receiver, state, at FROM \
             parcel WHERE swept_at IS NULL AND state IN (?1, ?2, ?3)",
            vec![
                state(State::Waiting)?,
                state(State::Ready)?,
                state(State::Refused)?,
            ],
        )
        .await?;
    let old = rows.at().0 - KEPT_MS;
    let mut changes = Vec::new();
    let mut swept = Vec::new();
    for (parcel, letter, sender, receiver, was, at) in unswept {
        let parcel = ParcelId(parcel);
        let sweeps = match change::from_case_name(was)? {
            State::Refused => true,
            State::Ready => {
                at < old
                    && !asking::still_held(rows, LetterId(letter))
                        .await?
            }
            State::Waiting => {
                let left = either_left(
                    rows,
                    AgentNumber(sender),
                    AgentNumber(receiver),
                )
                .await?;
                if left {
                    changes.push(Change::Refused { parcel }.into());
                }
                left
            }
            State::Measuring | State::Copying => false,
        };
        if sweeps {
            swept.push(parcel);
        }
    }
    if !swept.is_empty() {
        changes.push(Change::Swept { parcels: swept }.into());
    }
    Ok(changes)
}

async fn names_in(
    mut listed: tokio::fs::ReadDir,
) -> Result<Vec<String>> {
    let mut names = Vec::new();
    while let Some(entry) = listed.next_entry().await? {
        if let Ok(name) = entry.file_name().into_string() {
            names.push(name);
        }
    }
    Ok(names)
}

/// What a parcel's send's wait adds to the chief's `answer` on wait
/// `wait`: an allow begins the steps that copy and deliver it, and a
/// deny refuses it.
pub(super) async fn ruling(
    rows: &Rows<'_>,
    wait: WaitId,
    answer: &Answer,
) -> Result<Ruling> {
    let parcel = sending(rows, wait).await?;
    let changes = match answer.0.choice {
        Choice::Allow => vec![
            Change::Allowed { parcel }.into(),
            steps::Change::Begin {
                row: row(parcel, true),
            }
            .into(),
        ],
        Choice::Deny => vec![Change::Refused { parcel }.into()],
        _ => Vec::new(),
    };
    Ok(Ruling {
        changes,
        ..Ruling::default()
    })
}

/// The change a send's wait `wait` makes as it lapses: its parcel is
/// refused, for the sweep to take.
pub(super) async fn lapsed(
    rows: &Rows<'_>,
    wait: WaitId,
) -> Result<change::Change> {
    let parcel = sending(rows, wait).await?;
    Ok(Change::Refused { parcel }.into())
}

/// The open send waits of parcels to `receiver`.
pub(super) async fn waits_to(
    rows: &Rows<'_>,
    receiver: AgentNumber,
) -> Result<Vec<WaitId>> {
    let open = rows
        .rows::<(u64, Vec<u8>)>(
            "SELECT id, detail FROM wait WHERE kind = 'send' AND \
             closed_at IS NULL ORDER BY id",
            Vec::new(),
        )
        .await?;
    let mut waits = Vec::new();
    for (wait, detail) in open {
        let Sending { parcel, .. } =
            ciborium::from_reader(detail.as_slice())?;
        let to = rows
            .value::<u32>(
                "SELECT receiver FROM parcel WHERE id = ?1",
                vec![id(parcel)],
            )
            .await?;
        if to == Some(receiver.0) {
            waits.push(WaitId(wait));
        }
    }
    Ok(waits)
}

/// The parcel a send's wait `wait` waits on.
async fn sending(rows: &Rows<'_>, wait: WaitId) -> Result<ParcelId> {
    let detail = rows
        .value::<Vec<u8>>(
            "SELECT detail FROM wait WHERE id = ?1",
            vec![change::integer(wait.0)?],
        )
        .await?
        .with_context(|| {
            format!("send wait {} has no detail", wait.0)
        })?;
    let Sending { parcel, .. } =
        ciborium::from_reader(detail.as_slice())?;
    Ok(parcel)
}

/// The realm's folder of parcels' files, `local.files` beside its
/// database.
struct Store(PathBuf);

enum Kept {
    /// A file named by its hash.
    File,
    /// A file a copy of this parcel is writing.
    Temporary(u32),
    Other,
}

impl Store {
    fn of(ctx: &Ctx<Realm>) -> Result<Self> {
        let realm = address::Realm.address()?;
        Ok(Self(
            HostRef::of(ctx.inner())?
                .folder()
                .join("actors")
                .join(realm.kind())
                .join(format!("{}.files", realm.key())),
        ))
    }

    fn file(&self, sha256: &Sha256) -> PathBuf {
        self.0.join(sha256.to_string())
    }

    fn temporary(&self, parcel: ParcelId, file: usize) -> PathBuf {
        self.0.join(format!("{}.{file}.part", parcel.0))
    }

    fn kept_as(name: &str) -> Kept {
        if name.len() == 64
            && name.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Kept::File;
        }
        name.strip_suffix(".part")
            .and_then(|rest| rest.split_once('.'))
            .and_then(|(parcel, _)| parcel.parse().ok())
            .map_or(Kept::Other, Kept::Temporary)
    }
}
