//! Durable sends. An actor writes a send into its own file, in the
//! same transaction as the change that caused it, and the host
//! delivers it until the receiver confirms it, once.
//!
//! - **The sender.** [`send`] numbers the send within its pair, from
//!   1, and adds it to the sender's outbox, inside the sender's
//!   transaction.
//! - **The delivery.** Each generation of a sender has one
//!   [`Outbox`]. Its commit, its start and a pair's retry timer start
//!   one delivery task, which reads the committed outbox and delivers
//!   each pair's sends in order, one at a time, by calling the
//!   receiver's [`RECEIVE`] action with an envelope it builds from
//!   the sender's own file. A confirmed send leaves the outbox. A
//!   receiver with no address sets the send aside as a dead letter.
//! - **The receiver.** Its [`RECEIVE`] arm calls [`receive`], which
//!   applies the send in the receiver's own transaction, once, and
//!   keeps the answer until the sender has confirmed it. Every write
//!   of the receive path checks that the send it names is still the
//!   next of its pair (R2). A run counts as failed only once it has
//!   ended without success (R3). Each life of a sender key has its
//!   own position, so a key destroyed and made again is delivered
//!   once (R15).

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Weak};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use hamlet_actor_core::{
    ActorContext, BindParam, ColumnValue, SqliteDb, SqliteTransaction,
};
use parking_lot::Mutex;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use tokio::task::JoinSet;
use tokio::time::Instant;

use crate::cause::Cause;
use crate::directory::Address;
use crate::local::{HostInner, now_ms};
use crate::plain_sqlite::PlainSqlite;

/// The action that receives a send. The `hamlet.` prefix is the
/// host's: no caller outside the host reaches it (R5).
pub const RECEIVE: &str = "hamlet.receive";

/// Refuses an action under the host's prefix `hamlet.`: no caller
/// outside the host reaches one (R5).
pub(crate) fn refuse_reserved(action: &str) -> Result<()> {
    anyhow::ensure!(
        !action.starts_with("hamlet."),
        "the action `{action}` is under `hamlet.`, which only the \
         host calls"
    );
    Ok(())
}

/// How long a delivery waits for the receiver's answer. The core caps
/// it at the receiver kind's action timeout.
const DELIVERY_DEADLINE: Duration = Duration::from_secs(60);

/// How long a run of a send's handler may take: the core's
/// transaction timeout. A run past it has failed.
const RUN_TIMEOUT: Duration = Duration::from_secs(60);

/// The delay before a pair's first retry. It doubles with each
/// retry, up to [`LAST_RETRY`].
const FIRST_RETRY: Duration = Duration::from_millis(100);
const LAST_RETRY: Duration = Duration::from_secs(30);

/// The send tables of an actor file, and the file's own id, which its
/// first start makes and no later start changes. The start
/// transaction runs it before the core sees the file.
///
/// - `_hamlet_file`: the id of this file. A key destroyed and made
///   again is a new file with a new id, so a receiver tells the lives
///   of one sender key apart.
/// - `_hamlet_outbox`: sends this actor made that their receiver has
///   not confirmed, numbered from 1 per receiver.
/// - `_hamlet_peers`: per receiver, the next number to give and the
///   highest number it confirmed.
/// - `_hamlet_inbound`: per sender life, the highest number handled
///   here, the runs of the next one that ended without success, and
///   the generation of this file whose run of it is under way.
/// - `_hamlet_handled`: the answer to each send handled here, kept
///   until its sender has confirmed it.
/// - `_hamlet_dead_letters`: sends set aside, with the reason.
pub(crate) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS _hamlet_file (
	id INTEGER PRIMARY KEY CHECK (id = 0),
	file_id TEXT NOT NULL) STRICT;
INSERT OR IGNORE INTO _hamlet_file (id, file_id)
	VALUES (0, lower(hex(randomblob(16))));
CREATE TABLE IF NOT EXISTS _hamlet_outbox (
	to_kind TEXT NOT NULL,
	to_key TEXT NOT NULL,
	seq INTEGER NOT NULL,
	name TEXT NOT NULL,
	payload BLOB NOT NULL,
	task TEXT NOT NULL,
	cause TEXT,
	created_at INTEGER NOT NULL,
	PRIMARY KEY (to_kind, to_key, seq)) STRICT, WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS _hamlet_peers (
	kind TEXT NOT NULL,
	key TEXT NOT NULL,
	next_out INTEGER NOT NULL,
	acked_out INTEGER NOT NULL,
	PRIMARY KEY (kind, key)) STRICT, WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS _hamlet_inbound (
	kind TEXT NOT NULL,
	key TEXT NOT NULL,
	file_id TEXT NOT NULL,
	handled_in INTEGER NOT NULL,
	attempts_in INTEGER NOT NULL,
	running_in INTEGER,
	PRIMARY KEY (kind, key, file_id)) STRICT, WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS _hamlet_handled (
	kind TEXT NOT NULL,
	key TEXT NOT NULL,
	file_id TEXT NOT NULL,
	seq INTEGER NOT NULL,
	outcome BLOB NOT NULL,
	PRIMARY KEY (kind, key, file_id, seq)) STRICT, WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS _hamlet_dead_letters (
	id INTEGER PRIMARY KEY,
	peer_kind TEXT NOT NULL,
	peer_key TEXT NOT NULL,
	seq INTEGER,
	work_id TEXT,
	name TEXT NOT NULL,
	payload BLOB NOT NULL,
	task TEXT NOT NULL,
	cause TEXT,
	reason TEXT NOT NULL,
	attempts INTEGER NOT NULL,
	error TEXT,
	at INTEGER NOT NULL) STRICT;
";

/// Gives the next number of the pair, from 1.
const NEXT_SEQ: &str =
    "INSERT INTO _hamlet_peers (kind, key, next_out, acked_out) \
     VALUES (?1, ?2, 2, 0) ON CONFLICT (kind, key) DO UPDATE SET \
     next_out = next_out + 1 RETURNING next_out - 1";

/// The only statement that adds an outbox row. The generation's host
/// handle knows a send by it, and delivers once its transaction
/// commits.
pub(crate) const OUTBOX_INSERT: &str =
    "INSERT INTO _hamlet_outbox (to_kind, to_key, seq, name, \
     payload, task, cause, created_at) VALUES (?1, ?2, ?3, ?4, ?5, \
     ?6, ?7, ?8)";

/// Writes a send of `name` with `payload` to `to` into the sender's
/// outbox, inside `tx`, the transaction of the change that causes
/// it. The send commits with that change, or neither commits. The
/// host delivers it once `tx` commits. Returns its number within the
/// pair. A send never makes its receiver: one to a key with no file,
/// or to a kind the host does not hold, is set aside.
pub async fn send(
    tx: &SqliteTransaction,
    to: &Address,
    name: &str,
    payload: &[u8],
    cause: &Cause,
) -> Result<u64> {
    let numbered = tx
        .execute(NEXT_SEQ, Some(vec![text(&to.kind), text(&to.key)]))
        .await?;
    let seq =
        integer(&numbered.rows, 0).context("a send got no number")?;
    tx.execute(
        OUTBOX_INSERT,
        Some(vec![
            text(&to.kind),
            text(&to.key),
            BindParam::Integer(seq),
            text(name),
            BindParam::Blob(payload.to_vec()),
            text(cause.task()),
            cause.cause().map_or(BindParam::Null, text),
            BindParam::Integer(now_ms()),
        ]),
    )
    .await?;
    Ok(u64::try_from(seq)?)
}

/// What a delivery carries to [`RECEIVE`]. The host builds it from
/// the sender's own file: the sender never names itself.
#[derive(Serialize, Deserialize)]
struct Envelope {
    from_kind: String,
    from_key: String,
    /// The sender file's id: which life of the sender key sent it.
    from_file: String,
    seq: i64,
    /// The highest number of this pair the sender has confirmed.
    acked_through: i64,
    name: String,
    payload: Vec<u8>,
    task: String,
    cause: Option<String>,
}

/// The receiver's answer to a delivery.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Delivery {
    /// This delivery applied the send. `outcome` is what the handler
    /// returned.
    Handled { outcome: Vec<u8> },
    /// The send was applied before. `outcome` is the record's, or
    /// `None` once the record is pruned.
    Repeat { outcome: Option<Vec<u8>> },
    /// The send is not the next of its pair; the receiver expects
    /// `expected`.
    OutOfOrder { expected: u64 },
    /// The run did not apply the send, which is tried again.
    Failed { error: String },
}

impl Delivery {
    /// The answer as the bytes [`RECEIVE`] replies with.
    pub fn into_reply(self) -> Result<Vec<u8>> {
        let mut reply = Vec::new();
        ciborium::into_writer(&self, &mut reply)?;
        Ok(reply)
    }

    fn from_reply(reply: &[u8]) -> Result<Self> {
        ciborium::from_reader(reply)
            .context("decode a delivery's answer")
    }
}

/// A send as its receiver's handler sees it.
#[derive(Clone, Debug)]
pub struct Message {
    pub from: Address,
    /// The send's number within its pair.
    pub seq: u64,
    pub name: String,
    pub payload: Vec<u8>,
    /// Runs of this send's handler that ended without success before
    /// this one.
    pub failed_runs: u32,
    from_file: String,
    cause: Cause,
}

impl Message {
    /// The send's id: its sender's life and its number in the pair.
    pub fn id(&self) -> String {
        format!(
            "{}/{}@{}#{}",
            self.from.kind, self.from.key, self.from_file, self.seq
        )
    }

    /// Why the send was made.
    pub fn cause(&self) -> &Cause {
        &self.cause
    }

    /// The cause of a message sent while this one is handled: this
    /// send's task, and this send as the cause.
    pub fn follow(&self) -> Cause {
        Cause::new(self.cause.task(), Some(self.id()))
    }
}

/// Takes the send as the next of its pair, marked as run by this
/// generation, and returns the failed runs before it. A run that an
/// ended generation left marked is counted now. A row is made for a
/// sender life never received from: first contact takes the send it
/// is given as the first of the pair. Changes nothing, and returns no
/// row, for a send that is not the next.
///
/// A claim does not run the handler: the receive transaction reads
/// the count again, with [`RUNS`], as of its run.
const CLAIM: &str =
    "INSERT INTO _hamlet_inbound (kind, key, file_id, handled_in, \
     attempts_in, running_in) VALUES (?1, ?2, ?3, ?4 - 1, 0, \
     (SELECT generation FROM _hamlet_actor)) ON CONFLICT (kind, \
     key, file_id) DO UPDATE SET attempts_in = attempts_in + \
     (running_in IS NOT NULL AND running_in IS NOT \
     excluded.running_in), running_in = excluded.running_in WHERE \
     handled_in = excluded.handled_in RETURNING attempts_in";

/// The receive transaction's first statement: the failed runs of the
/// send, if it is still the next of its pair.
const RUNS: &str = "SELECT attempts_in FROM _hamlet_inbound WHERE \
                    kind = ?1 AND key = ?2 AND file_id = ?3 AND \
                    handled_in = ?4 - 1";

/// Marks the send handled if it is still the next of its pair.
const TAKE: &str = "UPDATE _hamlet_inbound SET handled_in = ?4, \
                    attempts_in = 0, running_in = NULL WHERE kind = \
                    ?1 AND key = ?2 AND file_id = ?3 AND handled_in \
                    = ?4 - 1";

/// Counts a run that ended without success, if its send is still the
/// next of its pair.
const FAIL: &str = "UPDATE _hamlet_inbound SET attempts_in = \
                    attempts_in + 1, running_in = NULL WHERE kind = \
                    ?1 AND key = ?2 AND file_id = ?3 AND handled_in \
                    = ?4 - 1";

/// Clears a claim whose run never began, counting nothing.
const UNCLAIM: &str = "UPDATE _hamlet_inbound SET running_in = NULL \
                       WHERE kind = ?1 AND key = ?2 AND file_id = \
                       ?3 AND handled_in = ?4 - 1";

/// The pair's position, and the record of the send, if kept.
const STANDING: &str =
    "SELECT handled_in, (SELECT outcome FROM _hamlet_handled WHERE \
     kind = ?1 AND key = ?2 AND file_id = ?3 AND seq = ?4) FROM \
     _hamlet_inbound WHERE kind = ?1 AND key = ?2 AND file_id = ?3";

const RECORD: &str = "INSERT INTO _hamlet_handled (kind, key, \
                      file_id, seq, outcome) VALUES (?1, ?2, ?3, \
                      ?4, ?5)";

/// Drops the records the sender has confirmed.
const PRUNE: &str = "DELETE FROM _hamlet_handled WHERE kind = ?1 \
                     AND key = ?2 AND file_id = ?3 AND seq <= ?4";

/// Receives the send in `envelope`, which the host delivered to the
/// receiver's [`RECEIVE`] action, and returns the answer to reply
/// with. `apply` runs the change the send asks for in the receive
/// transaction it is given, which it must neither commit nor roll
/// back; its result is the send's outcome. It runs at most once per
/// send that commits, however many deliveries of the send arrive, and
/// within [`RUN_TIMEOUT`]. A run that fails, panics, times out or is
/// dropped rolls back and is counted.
pub async fn receive<F, Fut>(
    ctx: &ActorContext,
    envelope: &[u8],
    apply: F,
) -> Result<Delivery>
where
    F: FnOnce(SqliteTransaction, Message) -> Fut,
    Fut: Future<Output = Result<Vec<u8>>>,
{
    let envelope: Envelope = ciborium::from_reader(envelope)
        .context("decode a send's envelope")?;
    // A run whose call timed out goes on: the actor must not sleep
    // under it (R3).
    ctx.keep_awake(receive_awake(ctx.sql().clone(), envelope, apply))
        .await
}

async fn receive_awake<F, Fut>(
    db: SqliteDb,
    envelope: Envelope,
    apply: F,
) -> Result<Delivery>
where
    F: FnOnce(SqliteTransaction, Message) -> Fut,
    Fut: Future<Output = Result<Vec<u8>>>,
{
    let Envelope {
        from_kind,
        from_key,
        from_file,
        seq,
        acked_through,
        name,
        payload,
        task,
        cause,
    } = envelope;
    let pair = Position {
        from: Address::new(from_kind, from_key),
        from_file,
        seq,
    };
    let claimed =
        db.execute(CLAIM.to_owned(), Some(pair.params())).await?;
    if claimed.rows.is_empty() {
        return standing(&db, &pair).await;
    }
    let tx = match db.begin_transaction(None).await {
        Ok(tx) => tx,
        Err(error) => {
            return Ok(uncounted(&db, None, &pair, error).await);
        }
    };
    let run = Run {
        db: db.clone(),
        tx: tx.clone(),
        pair: pair.clone(),
        armed: true,
    };
    let taken = async {
        let runs = tx.execute(RUNS, Some(pair.params())).await?;
        let Some(failed_runs) = integer(&runs.rows, 0) else {
            return Ok(None);
        };
        let taken = tx.execute(TAKE, Some(pair.params())).await?;
        Ok((taken.changes == 1).then_some(failed_runs))
    }
    .await;
    let failed_runs = match taken {
        Ok(Some(failed_runs)) => failed_runs,
        // Another delivery of the send ran it meanwhile.
        Ok(None) => {
            run.disarm();
            tx.rollback().await?;
            return standing(&db, &pair).await;
        }
        Err(error) => {
            run.disarm();
            return Ok(uncounted(&db, Some(&tx), &pair, error).await);
        }
    };
    let message = Message {
        from: pair.from.clone(),
        seq: u64::try_from(seq)?,
        name,
        payload,
        failed_runs: u32::try_from(failed_runs)?,
        from_file: pair.from_file.clone(),
        cause: Cause::new(task, cause),
    };
    let ran =
        tokio::time::timeout(RUN_TIMEOUT, apply(tx.clone(), message))
            .await
            .unwrap_or_else(|_| {
                Err(anyhow!("the handler ran past {RUN_TIMEOUT:?}"))
            });
    let outcome = match ran {
        Ok(outcome) => outcome,
        Err(error) => return Ok(run.failed(error).await),
    };
    let finished = async {
        let mut record = pair.params();
        record.push(BindParam::Blob(outcome.clone()));
        tx.execute(RECORD, Some(record)).await?;
        let mut prune = pair.params();
        prune[3] = BindParam::Integer(acked_through);
        tx.execute(PRUNE, Some(prune)).await?;
        tx.commit().await
    }
    .await;
    match finished {
        Ok(()) => {
            run.disarm();
            Ok(Delivery::Handled { outcome })
        }
        Err(error) => Ok(run.failed(error).await),
    }
}

/// The send a receive names, in its sender's life.
#[derive(Clone)]
struct Position {
    from: Address,
    from_file: String,
    seq: i64,
}

impl Position {
    /// `?1` to `?4` of the receive statements.
    fn params(&self) -> Vec<BindParam> {
        vec![
            text(&self.from.kind),
            text(&self.from.key),
            text(&self.from_file),
            BindParam::Integer(self.seq),
        ]
    }
}

/// The answer to a send that is not the next of its pair: a repeat,
/// with its record if kept, or out of order.
async fn standing(
    db: &SqliteDb,
    pair: &Position,
) -> Result<Delivery> {
    let read =
        db.execute(STANDING.to_owned(), Some(pair.params())).await?;
    let handled_in = integer(&read.rows, 0)
        .context("a send that is not next has no position")?;
    if pair.seq > handled_in {
        return Ok(Delivery::OutOfOrder {
            expected: u64::try_from(handled_in + 1)?,
        });
    }
    tracing::info!(
        "repeat {}/{} {}",
        pair.from.kind,
        pair.from.key,
        pair.seq
    );
    let outcome = match read.rows.first().and_then(|row| row.get(1)) {
        Some(ColumnValue::Blob(outcome)) => Some(outcome.clone()),
        _ => None,
    };
    Ok(Delivery::Repeat { outcome })
}

/// Ends a receive whose run never began: rolls back `tx` if one is
/// open, and clears the claim without counting.
async fn uncounted(
    db: &SqliteDb,
    tx: Option<&SqliteTransaction>,
    pair: &Position,
    error: anyhow::Error,
) -> Delivery {
    if let Some(tx) = tx {
        let _ = tx.rollback().await;
    }
    if let Err(clear) =
        db.execute(UNCLAIM.to_owned(), Some(pair.params())).await
    {
        tracing::warn!(?clear, "a receive left its claim marked");
    }
    Delivery::Failed {
        error: format!("{error:#}"),
    }
}

/// A run of a send's handler, from its transaction's guard to its
/// end. Dropped while armed, as by a panic or a dropped receive, it
/// ends as a failed run on a task of its own.
struct Run {
    db: SqliteDb,
    tx: SqliteTransaction,
    pair: Position,
    armed: bool,
}

impl Run {
    fn disarm(mut self) {
        self.armed = false;
    }

    /// Ends the run as failed: rolls back, then counts it.
    async fn failed(mut self, error: anyhow::Error) -> Delivery {
        self.armed = false;
        count_failed(&self.db, &self.tx, &self.pair).await;
        Delivery::Failed {
            error: format!("{error:#}"),
        }
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        if self.armed {
            let (db, tx, pair) =
                (self.db.clone(), self.tx.clone(), self.pair.clone());
            tokio::spawn(async move {
                count_failed(&db, &tx, &pair).await;
            });
        }
    }
}

/// Rolls the run's transaction back, so the failure write does not
/// wait behind it, and counts the run.
async fn count_failed(
    db: &SqliteDb,
    tx: &SqliteTransaction,
    pair: &Position,
) {
    let _ = tx.rollback().await;
    if let Err(error) =
        db.execute(FAIL.to_owned(), Some(pair.params())).await
    {
        tracing::warn!(
            ?error,
            "a failed run of a send was not counted"
        );
    }
}

/// The outbox of one sender generation, and its delivery. The
/// generation's host handle marks it when a send's insert passes
/// through it and starts the delivery when the transaction commits;
/// the start of the generation starts it too.
pub(crate) struct Outbox {
    host: Weak<HostInner>,
    from: Address,
    file: PlainSqlite,
    state: Mutex<Delivering>,
}

#[derive(Default)]
struct Delivering {
    /// The outbox may hold sends a delivery task has not read.
    dirty: bool,
    /// A delivery task runs. It reads the outbox again before it
    /// ends if `dirty` is set.
    running: bool,
    /// Pairs whose last delivery failed, with their next try.
    retries: HashMap<Address, Retry>,
}

#[derive(Clone, Copy)]
struct Retry {
    delay: Duration,
    at: Instant,
}

/// The committed outbox, as one delivery task reads it.
struct Pending {
    from_file: String,
    pairs: Vec<PendingPair>,
}

struct PendingPair {
    to: Address,
    acked: i64,
    sends: Vec<Sent>,
}

struct Sent {
    seq: i64,
    name: String,
    payload: Vec<u8>,
    task: String,
    cause: Option<String>,
}

/// How a delivery of one send ended.
enum Step {
    Confirmed,
    /// Try the pair again later.
    Again,
    /// The sender's file is closed, or its host is gone.
    Closed,
}

impl Outbox {
    pub(crate) fn new(
        host: Weak<HostInner>,
        from: Address,
        file: PlainSqlite,
    ) -> Arc<Self> {
        Arc::new(Self {
            host,
            from,
            file,
            state: Mutex::default(),
        })
    }

    /// A send's insert passed: the outbox may hold a new row.
    pub(crate) fn mark(&self) {
        self.state.lock().dirty = true;
    }

    /// A transaction of the generation committed. Delivers what a
    /// send in it added.
    pub(crate) fn committed(self: &Arc<Self>) {
        if self.state.lock().dirty {
            crate::fault::point("send-committed");
            self.start();
        }
    }

    /// Delivers the outbox: starts the delivery task, or makes the
    /// one that runs read the outbox again.
    pub(crate) fn start(self: &Arc<Self>) {
        let mut state = self.state.lock();
        state.dirty = true;
        if state.running {
            return;
        }
        state.running = true;
        drop(state);
        tokio::spawn(self.clone().deliver());
    }

    async fn deliver(self: Arc<Self>) {
        let mut running = RunningMark(Some(self.clone()));
        loop {
            self.state.lock().dirty = false;
            let open = self.round().await;
            // One lock decides to end and clears `running`, so a mark
            // made after the read is never left undelivered.
            let mut state = self.state.lock();
            if open && state.dirty {
                continue;
            }
            state.running = false;
            running.0 = None;
            return;
        }
    }

    /// Delivers each pair the outbox holds, the pairs at once, the
    /// sends of a pair in order. Returns false once the outbox can
    /// deliver nothing more: its file is closed, or its host is
    /// dropped or shut.
    async fn round(self: &Arc<Self>) -> bool {
        if !self.host_is_open() {
            return false;
        }
        // Held until the sender has no transaction open, so it reads
        // only committed sends.
        let Ok(pending) =
            self.file.between_transactions(read_outbox).await
        else {
            return false;
        };
        let now = Instant::now();
        let mut pairs = JoinSet::new();
        for pair in pending.pairs {
            let waiting = self
                .state
                .lock()
                .retries
                .get(&pair.to)
                .is_some_and(|retry| retry.at > now);
            if !waiting {
                pairs.spawn(
                    self.clone().deliver_pair(
                        pending.from_file.clone(),
                        pair,
                    ),
                );
            }
        }
        let mut open = true;
        while let Some(ended) = pairs.join_next().await {
            open &= ended.unwrap_or(true);
        }
        open
    }

    /// Delivers the sends of one pair in order, each once confirmed
    /// before the next. Returns false if the sender's file is closed.
    async fn deliver_pair(
        self: Arc<Self>,
        from_file: String,
        pair: PendingPair,
    ) -> bool {
        let PendingPair {
            to,
            mut acked,
            sends,
        } = pair;
        for sent in sends {
            let seq = sent.seq;
            match self.deliver_one(&from_file, &to, acked, sent).await
            {
                Step::Confirmed => {
                    acked = seq;
                    self.state.lock().retries.remove(&to);
                }
                Step::Again => {
                    self.retry(&to);
                    return true;
                }
                Step::Closed => return false,
            }
        }
        true
    }

    async fn deliver_one(
        &self,
        from_file: &str,
        to: &Address,
        acked: i64,
        sent: Sent,
    ) -> Step {
        let Some(host) = self.host.upgrade() else {
            return Step::Closed;
        };
        let receiver = host.receiver(to).await;
        // The host's lock on its folder is not held across the call.
        drop(host);
        let seq = sent.seq;
        let generation = match receiver {
            Ok(Some(generation)) => generation,
            Ok(None) => {
                return match self.set_aside(to, seq).await {
                    Ok(()) => Step::Confirmed,
                    Err(_) => Step::Closed,
                };
            }
            Err(error) => {
                tracing::debug!(
                    ?error,
                    ?to,
                    seq,
                    "a send's receiver did not start"
                );
                return Step::Again;
            }
        };
        let envelope = Envelope {
            from_kind: self.from.kind.clone(),
            from_key: self.from.key.clone(),
            from_file: from_file.to_owned(),
            seq,
            acked_through: acked,
            name: sent.name,
            payload: sent.payload,
            task: sent.task,
            cause: sent.cause,
        };
        let mut bytes = Vec::new();
        if let Err(error) =
            ciborium::into_writer(&envelope, &mut bytes)
        {
            tracing::error!(
                ?error,
                "a send's envelope did not encode"
            );
            return Step::Again;
        }
        let answer = generation
            .actor
            .action(RECEIVE, bytes, DELIVERY_DEADLINE)
            .await
            .and_then(|reply| Delivery::from_reply(&reply));
        match answer {
            Ok(
                Delivery::Handled { .. } | Delivery::Repeat { .. },
            ) => match self.ack(to, seq).await {
                Ok(()) => Step::Confirmed,
                Err(_) => Step::Closed,
            },
            Ok(Delivery::OutOfOrder { expected }) => {
                tracing::warn!(
                    ?to,
                    seq,
                    expected,
                    "a send is out of order"
                );
                Step::Again
            }
            Ok(Delivery::Failed { error }) => {
                tracing::debug!(
                    error,
                    ?to,
                    seq,
                    "a send's run failed"
                );
                Step::Again
            }
            Err(error) => {
                tracing::debug!(
                    ?error,
                    ?to,
                    seq,
                    "a send's delivery failed"
                );
                Step::Again
            }
        }
    }

    /// Schedules the pair's next try, its delay doubled from the
    /// last.
    fn retry(self: &Arc<Self>, to: &Address) {
        if !self.host_is_open() {
            return;
        }
        let delay = {
            let mut state = self.state.lock();
            let delay =
                state.retries.get(to).map_or(FIRST_RETRY, |retry| {
                    (retry.delay * 2).min(LAST_RETRY)
                });
            state.retries.insert(
                to.clone(),
                Retry {
                    delay,
                    at: Instant::now() + delay,
                },
            );
            delay
        };
        let outbox = Arc::downgrade(self);
        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            if let Some(outbox) = outbox.upgrade() {
                outbox.start();
            }
        });
    }

    fn host_is_open(&self) -> bool {
        self.host.upgrade().is_some_and(|host| !host.is_shut())
    }

    /// The receiver confirmed the send: it leaves the outbox, and the
    /// pair's confirmed number moves to it, in one host write.
    async fn ack(&self, to: &Address, seq: i64) -> Result<()> {
        let to = to.clone();
        self.file
            .between_transactions(move |conn| {
                crate::fault::point("send-acked");
                let tx = conn.unchecked_transaction()?;
                confirm(&tx, &to, seq)?;
                tx.commit()?;
                Ok(())
            })
            .await
    }

    /// The send has no address: it moves from the outbox to the dead
    /// letters with the reason `no_address`, in one host write.
    async fn set_aside(&self, to: &Address, seq: i64) -> Result<()> {
        let moved = to.clone();
        self.file
            .between_transactions(move |conn| {
                let tx = conn.unchecked_transaction()?;
                tx.execute(
                    "INSERT INTO _hamlet_dead_letters (peer_kind, \
                     peer_key, seq, name, payload, task, cause, \
                     reason, attempts, at) SELECT to_kind, to_key, \
                     seq, name, payload, task, cause, 'no_address', \
                     0, ?4 FROM _hamlet_outbox WHERE to_kind = ?1 \
                     AND to_key = ?2 AND seq = ?3",
                    params![moved.kind, moved.key, seq, now_ms()],
                )?;
                confirm(&tx, &moved, seq)?;
                tx.commit()?;
                Ok(())
            })
            .await?;
        report_dead_letter(&self.from, to, seq);
        Ok(())
    }
}

/// The deadline a stopping sender leaves for what its outbox still
/// holds: now, if it holds a send, so the sender starts again and
/// delivers it. A stop is safe at any point of a delivery: what was
/// confirmed has left the outbox, and the receiver answers a send it
/// applied again from its record.
pub(crate) fn undelivered(conn: &Connection) -> Result<Option<i64>> {
    let holds: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM _hamlet_outbox)",
        [],
        |row| row.get(0),
    )?;
    Ok(holds.then(now_ms))
}

/// Stand-in for the host's dead-letter event, until `Host::events()`
/// exists: a warning in the host's log.
fn report_dead_letter(from: &Address, to: &Address, seq: i64) {
    tracing::warn!(
        ?from,
        ?to,
        seq,
        "a send has no address and is set aside"
    );
}

/// Ends the delivery task's `running` mark if the task ends without
/// deciding to, as by a panic.
struct RunningMark(Option<Arc<Outbox>>);

impl Drop for RunningMark {
    fn drop(&mut self) {
        if let Some(outbox) = &self.0 {
            outbox.state.lock().running = false;
        }
    }
}

/// Drops the send from the outbox and moves the pair's confirmed
/// number to it.
fn confirm(conn: &Connection, to: &Address, seq: i64) -> Result<()> {
    conn.execute(
        "DELETE FROM _hamlet_outbox WHERE to_kind = ?1 AND to_key = \
         ?2 AND seq = ?3",
        params![to.kind, to.key, seq],
    )?;
    conn.execute(
        "UPDATE _hamlet_peers SET acked_out = MAX(acked_out, ?3) \
         WHERE kind = ?1 AND key = ?2",
        params![to.kind, to.key, seq],
    )?;
    Ok(())
}

/// The committed outbox, by pair and in order, with the file's id.
fn read_outbox(conn: &Connection) -> Result<Pending> {
    let from_file: String = conn.query_row(
        "SELECT file_id FROM _hamlet_file",
        [],
        |row| row.get(0),
    )?;
    let mut statement = conn.prepare(
        "SELECT o.to_kind, o.to_key, o.seq, o.name, o.payload, \
         o.task, o.cause, COALESCE(p.acked_out, 0) FROM \
         _hamlet_outbox o LEFT JOIN _hamlet_peers p ON p.kind = \
         o.to_kind AND p.key = o.to_key ORDER BY o.to_kind, \
         o.to_key, o.seq",
    )?;
    let mut rows = statement.query([])?;
    let mut pairs: Vec<PendingPair> = Vec::new();
    while let Some(row) = rows.next()? {
        let to = Address::new(
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
        );
        let sent = Sent {
            seq: row.get(2)?,
            name: row.get(3)?,
            payload: row.get(4)?,
            task: row.get(5)?,
            cause: row.get(6)?,
        };
        match pairs.last_mut() {
            Some(pair) if pair.to == to => pair.sends.push(sent),
            _ => pairs.push(PendingPair {
                to,
                acked: row.get(7)?,
                sends: vec![sent],
            }),
        }
    }
    Ok(Pending { from_file, pairs })
}

fn text(value: &str) -> BindParam {
    BindParam::Text(value.to_owned())
}

/// The integer in column `column` of the first row.
fn integer(rows: &[Vec<ColumnValue>], column: usize) -> Option<i64> {
    match rows.first()?.get(column)? {
        ColumnValue::Integer(value) => Some(*value),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use rusqlite::OptionalExtension;

    use super::*;

    /// A receiver's file with the send tables, in its generation 1.
    fn receiver() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE _hamlet_actor (id INTEGER PRIMARY KEY, \
             generation INTEGER NOT NULL); INSERT INTO \
             _hamlet_actor VALUES (0, 1)",
        )
        .unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        conn
    }

    /// The receive statement `sql` for send `seq` of life `life` of
    /// sender `counter/a`. Returns the rows it changed.
    fn run(
        conn: &Connection,
        sql: &str,
        life: &str,
        seq: i64,
    ) -> usize {
        conn.execute(sql, params!["counter", "a", life, seq])
            .unwrap()
    }

    /// The claim of send `seq` of `life`: its failed runs, or `None`.
    fn claim(conn: &Connection, life: &str, seq: i64) -> Option<i64> {
        conn.query_row(
            CLAIM,
            params!["counter", "a", life, seq],
            |row| row.get(0),
        )
        .optional()
        .unwrap()
    }

    /// The pair's `(handled_in, attempts_in)` for `life`.
    fn position(conn: &Connection, life: &str) -> (i64, i64) {
        conn.query_row(
            "SELECT handled_in, attempts_in FROM _hamlet_inbound \
             WHERE file_id = ?",
            [life],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap()
    }

    fn next_generation(conn: &Connection) {
        conn.execute(
            "UPDATE _hamlet_actor SET generation = generation + 1",
            [],
        )
        .unwrap();
    }

    /// R2: two deliveries of one send both find it next; one is
    /// handled; then the other's guard, its failure write and a late
    /// claim change nothing, and the next send starts with no failed
    /// run.
    #[test]
    fn a_lost_race_of_two_deliveries_writes_nothing() {
        let conn = receiver();
        assert_eq!(claim(&conn, "f1", 1), Some(0));
        assert_eq!(claim(&conn, "f1", 1), Some(0));
        assert_eq!(run(&conn, TAKE, "f1", 1), 1);
        assert_eq!(run(&conn, TAKE, "f1", 1), 0);
        assert_eq!(run(&conn, FAIL, "f1", 1), 0);
        assert_eq!(run(&conn, UNCLAIM, "f1", 1), 0);
        assert_eq!(claim(&conn, "f1", 1), None);
        assert_eq!(position(&conn, "f1"), (1, 0));
        assert_eq!(claim(&conn, "f1", 2), Some(0));
    }

    /// R3: a run an ended generation left marked is counted once, by
    /// the next generation's first claim; a second claim in that
    /// generation counts nothing, and a failed run counts itself.
    #[test]
    fn a_cut_run_and_a_failed_run_count_once_each() {
        let conn = receiver();
        assert_eq!(claim(&conn, "f1", 1), Some(0));
        next_generation(&conn);
        assert_eq!(claim(&conn, "f1", 1), Some(1));
        assert_eq!(claim(&conn, "f1", 1), Some(1));
        assert_eq!(run(&conn, FAIL, "f1", 1), 1);
        assert_eq!(claim(&conn, "f1", 1), Some(2));
        assert_eq!(run(&conn, UNCLAIM, "f1", 1), 1);
        next_generation(&conn);
        assert_eq!(claim(&conn, "f1", 1), Some(2));
    }

    /// First contact takes the send it is given; a later send is out
    /// of order and changes nothing.
    #[test]
    fn first_contact_takes_any_number_and_then_keeps_order() {
        let conn = receiver();
        assert_eq!(claim(&conn, "f1", 7), Some(0));
        assert_eq!(run(&conn, TAKE, "f1", 7), 1);
        assert_eq!(claim(&conn, "f1", 9), None);
        assert_eq!(position(&conn, "f1"), (7, 0));
    }

    /// R15: the old life's delivery is in flight when the key is made
    /// again; the new life's first send is handled and its answer
    /// lost; the old delivery lands; the new life's retry is a
    /// repeat, and its next send is taken.
    #[test]
    fn each_life_of_a_sender_keeps_its_own_position() {
        let conn = receiver();
        assert_eq!(claim(&conn, "old", 1), Some(0));
        assert_eq!(run(&conn, TAKE, "old", 1), 1);
        assert_eq!(claim(&conn, "old", 2), Some(0));

        assert_eq!(claim(&conn, "new", 1), Some(0));
        assert_eq!(run(&conn, TAKE, "new", 1), 1);
        assert_eq!(run(&conn, TAKE, "old", 2), 1);

        assert_eq!(claim(&conn, "new", 1), None);
        assert_eq!(position(&conn, "new"), (1, 0));
        assert_eq!(claim(&conn, "new", 2), Some(0));
        assert_eq!(position(&conn, "old"), (2, 0));
    }
}
