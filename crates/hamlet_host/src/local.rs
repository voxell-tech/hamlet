//! Runs actors in this process. Each actor is one plain SQLite file
//! at `actors/<kind>/<key>.sqlite` under the realm folder. The host
//! finds an actor by kind and key, starts it, stops it, and keeps its
//! next alarm in the file's `_hamlet_actor` row, so an actor whose
//! alarm is due starts when the host boots.
//!
//! The stored alarm trails the actor's own schedule: the core goes on
//! once the alarm is queued, so an action can reply before the host
//! stored it. The row also marks an actor running from its start
//! until a clean stop, and boot starts again each actor a kill left
//! marked. That start takes the next alarm from the actor's own
//! schedule and stores it again.
//!
//! Each actor's place in its lifecycle is one [`Life`] in the host's
//! map. One actor's start or stop waits only on that actor's work.

use std::collections::HashMap;
use std::fs::{File, TryLockError};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};

use anyhow::{Context, Result, anyhow, bail};
use hamlet_actor_core::host::{
    ActorHost, RunningActor, StartRequest,
};
use hamlet_actor_core::sqlite::{
    BindParam, ExecuteResult, QueryResult, SqliteBatchStatement,
};
use hamlet_actor_core::{
    ActorFactory, ActorKeySegment, RuntimeBoxFuture, ShutdownKind,
};
use parking_lot::Mutex;
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension};
use tokio::sync::{broadcast, oneshot, watch};
use tokio::task::AbortHandle;

use crate::plain_sqlite::{self, PlainSqlite};

/// The one session a local file has. The transaction code compares
/// sessions to detect a lost connection, which a local file cannot
/// have.
const LOCAL_SESSION: u64 = 1;

/// The host's row in each actor file. Its version ladder is Hamlet's,
/// apart from the core's `_actor_` tables.
const ACTOR_TABLE: &str = "CREATE TABLE IF NOT EXISTS _hamlet_actor \
                           (
	id INTEGER PRIMARY KEY CHECK (id = 0),
	kind TEXT NOT NULL,
	key TEXT NOT NULL,
	generation INTEGER NOT NULL,
	wake_at INTEGER,
	running INTEGER NOT NULL DEFAULT 0)";

/// Where one actor is in its lifecycle. The map holds no entry for an
/// actor that is stopped: none of its files is open.
///
/// Every transition takes the map's lock, checks the state, writes
/// the next one and releases the lock. The lock is never held across
/// an await, a file operation or a blocking send. The work a
/// transition begins, a start or a stop, runs on a task of its own,
/// which owns the state through [`Owned`] and makes the closing
/// transition when it ends, however it ends. A caller that waits
/// clones the state's receiver under the lock, awaits it after, and
/// reads the map again once it closes.
///
/// | From | Event | To |
/// |---|---|---|
/// | absent | a start: `get_or_start`, an alarm, boot | `Starting` |
/// | `Starting` | the start transaction ran | `Starting`, with its generation |
/// | `Starting` | its generation asks to sleep or to be destroyed, or reports a crash | `Starting`, with the request kept; a destroy wins over a sleep |
/// | `Starting` | the start ended well | `Running`, or `Stopping` or `Destroying` for a kept request |
/// | `Starting` | the start failed or panicked | absent |
/// | `Running` | its generation asks to sleep or reports a crash, or `Host::stop(Sleep)` | `Stopping` |
/// | `Running` | its generation asks to be destroyed, or `Host::stop(Destroy)` | `Destroying` |
/// | `Stopping` | its generation asks to be destroyed, or `Host::stop(Destroy)` | `Destroying` |
/// | `Stopping` | its generation asks to sleep or reports a crash, or `Host::stop(Sleep)` | unchanged |
/// | `Destroying` | any request | unchanged |
/// | `Stopping`, `Destroying` | the stop ended, however it ended | absent |
/// | any | a request from a generation the state does not hold | unchanged |
///
/// Callers wait without a transition of their own. A start waits for
/// a `Starting` or `Stopping` actor and then starts it, and fails at
/// once for a `Destroying` one. An action waits for a `Starting`
/// actor and fails for one that is not `Running`. `Host::stop` waits
/// for a `Starting` actor and then stops it, and waits for a stop
/// under way.
enum Life {
    /// Opening the file and starting the core. `generation` is set
    /// once the start transaction has raised it; `request` is a stop
    /// that generation asked for before its start ended.
    Starting {
        generation: Option<u32>,
        request: Option<ShutdownKind>,
        ready: watch::Receiver<()>,
    },
    Running(Arc<Generation>),
    /// A sleep, or a crash, of this generation is under way.
    Stopping {
        generation: Arc<Generation>,
        done: watch::Receiver<()>,
    },
    /// This generation stops, then its file is removed.
    Destroying {
        generation: Arc<Generation>,
        done: watch::Receiver<()>,
    },
}

impl Life {
    /// The generation that holds the actor's file: from the end of
    /// its start until its stop closed the file.
    fn loaded(&self) -> Option<&Arc<Generation>> {
        match self {
            Self::Starting { .. } => None,
            Self::Running(generation)
            | Self::Stopping { generation, .. }
            | Self::Destroying { generation, .. } => Some(generation),
        }
    }

    /// Whether the state belongs to generation `number`.
    fn is_generation(&self, number: u32) -> bool {
        match self {
            Self::Starting { generation, .. } => {
                *generation == Some(number)
            }
            _ => self.loaded().is_some_and(|generation| {
                generation.number == number
            }),
        }
    }

    /// What a waiter on this state awaits: the receiver closes when
    /// the work that owns the state ends.
    fn waiters(&self) -> Option<&watch::Receiver<()>> {
        match self {
            Self::Running(_) => None,
            Self::Starting { ready: waiters, .. }
            | Self::Stopping { done: waiters, .. }
            | Self::Destroying { done: waiters, .. } => Some(waiters),
        }
    }
}

/// One start of an actor, from its start transaction until its stop
/// closed the file.
struct Generation {
    number: u32,
    actor: RunningActor,
    file: PlainSqlite,
}

/// The work that owns an actor's `Starting`, `Stopping` or
/// `Destroying` state. When it is dropped, however its work ended, it
/// removes the state from the map if the state is still its own, and
/// then wakes the state's waiters.
struct Owned {
    host: Arc<HostInner>,
    actor_id: String,
    /// Wakes the waiters when it drops, after [`Drop::drop`].
    _wake: watch::Sender<()>,
    waiters: watch::Receiver<()>,
}

impl Owned {
    fn new(host: &Arc<HostInner>, actor_id: &str) -> Self {
        let (wake, waiters) = watch::channel(());
        Self {
            host: host.clone(),
            actor_id: actor_id.to_owned(),
            _wake: wake,
            waiters,
        }
    }

    fn owns(&self, life: &Life) -> bool {
        life.waiters().is_some_and(|waiters| {
            waiters.same_channel(&self.waiters)
        })
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        let removed = {
            let mut actors = self.host.actors.lock();
            actors
                .get(&self.actor_id)
                .is_some_and(|life| self.owns(life))
                .then(|| actors.remove(&self.actor_id))
        };
        // Dropped after the lock: it may hold the generation's last
        // reference.
        drop(removed);
    }
}

/// A stop that a transition began, to run on a task of its own.
struct Stop {
    owned: Owned,
    generation: Arc<Generation>,
    reason: ShutdownKind,
}

impl Stop {
    /// Stops the generation and closes its file, and removes the file
    /// if the actor is destroyed by then. The actor leaves the map
    /// when `owned` drops at the end.
    async fn run(self) -> Result<()> {
        let Self {
            owned,
            generation,
            reason,
        } = self;
        generation.actor.stop(reason).await;
        // Queued behind every alarm the actor stored, so the mark
        // goes only once they are in the file.
        let cleared = generation.file.host_write(
            "run-ended",
            "UPDATE _hamlet_actor SET running = 0",
            Vec::new(),
        );
        generation.file.close().await;
        if let Err(error) = async { cleared?.wait().await }.await {
            tracing::error!(
                ?error,
                actor_id = owned.actor_id,
                "host failed to clear the running mark"
            );
        }
        let host = &owned.host;
        let destroyed =
            host.actors.lock().get(&owned.actor_id).is_some_and(
                |life| {
                    matches!(life, Life::Destroying { .. })
                        && owned.owns(life)
                },
            );
        if destroyed {
            host.alarms.lock().remove(&owned.actor_id);
            let path = actor_path(&host.dir, &owned.actor_id);
            std::fs::remove_file(&path).with_context(|| {
                format!("remove {}", path.display())
            })?;
        }
        Ok(())
    }

    /// Runs the stop on a task of its own and logs its error. The
    /// actor itself asked for it, so no caller waits.
    fn spawn_logged(self) {
        let actor_id = self.owned.actor_id.clone();
        tokio::spawn(async move {
            if let Err(error) = self.run().await {
                tracing::error!(
                    ?error,
                    actor_id,
                    "host failed to stop actor"
                );
            }
        });
    }
}

struct HostInner {
    dir: PathBuf,
    /// Held open for the host's life: its lock keeps a second host
    /// out of the folder.
    _lock: File,
    factories: HashMap<String, Arc<ActorFactory>>,
    /// Each actor that is not stopped, by id.
    actors: Mutex<HashMap<String, Life>>,
    /// Each actor's next alarm, as last stored in its file.
    alarms: Mutex<HashMap<String, i64>>,
    /// Each actor's one armed alarm timer.
    timers: Mutex<HashMap<String, AbortHandle>>,
    session: watch::Sender<u64>,
    commits: broadcast::Sender<String>,
}

impl HostInner {
    /// Asks the actor to stop: generation `generation` when the actor
    /// asks, whichever generation runs when the host asks. Makes the
    /// transition, and returns the stop to run if one begins.
    fn ask_stop(
        self: &Arc<Self>,
        actor_id: &str,
        generation: Option<u32>,
        reason: ShutdownKind,
    ) -> Option<Stop> {
        let mut actors = self.actors.lock();
        let life = actors.get_mut(actor_id)?;
        if generation
            .is_some_and(|number| !life.is_generation(number))
        {
            return None;
        }
        match life {
            Life::Starting { request, .. } => {
                if generation.is_some()
                    && !matches!(request, Some(ShutdownKind::Destroy))
                {
                    *request = Some(reason);
                }
                None
            }
            Life::Running(running) => {
                let generation = running.clone();
                let owned = Owned::new(self, actor_id);
                *life = stopping(&owned, generation.clone(), reason);
                Some(Stop {
                    owned,
                    generation,
                    reason,
                })
            }
            Life::Stopping { generation, done }
                if matches!(reason, ShutdownKind::Destroy) =>
            {
                *life = Life::Destroying {
                    generation: generation.clone(),
                    done: done.clone(),
                };
                None
            }
            Life::Stopping { .. } | Life::Destroying { .. } => None,
        }
    }
}

/// The state of `generation` while `owned` stops it for `reason`.
fn stopping(
    owned: &Owned,
    generation: Arc<Generation>,
    reason: ShutdownKind,
) -> Life {
    let done = owned.waiters.clone();
    match reason {
        ShutdownKind::Sleep => Life::Stopping { generation, done },
        ShutdownKind::Destroy => {
            Life::Destroying { generation, done }
        }
    }
}

/// The actors of one realm folder, run in this process.
#[derive(Clone)]
pub struct Host(Arc<HostInner>);

/// The core's handle on the host for one generation of one actor. It
/// reaches only that generation: its SQL goes to the generation's own
/// file, and its requests act only while the map holds that
/// generation.
struct LocalHandle {
    host: Weak<HostInner>,
    actor_id: String,
    generation: u32,
    file: PlainSqlite,
}

impl Host {
    /// Takes the folder's lock and opens it. Fails when another host
    /// holds the lock.
    pub fn open(
        dir: &Path,
        factories: HashMap<String, ActorFactory>,
    ) -> Result<Self> {
        std::fs::create_dir_all(dir.join("actors"))?;
        let lock_path = dir.join("host.lock");
        let lock = File::create(&lock_path).with_context(|| {
            format!("open {}", lock_path.display())
        })?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => bail!(
                "another host holds the lock {}",
                lock_path.display()
            ),
            Err(TryLockError::Error(error)) => {
                return Err(error).with_context(|| {
                    format!("lock {}", lock_path.display())
                });
            }
        }
        Ok(Self(Arc::new(HostInner {
            dir: dir.to_owned(),
            _lock: lock,
            factories: factories
                .into_iter()
                .map(|(kind, factory)| (kind, Arc::new(factory)))
                .collect(),
            actors: Mutex::new(HashMap::new()),
            alarms: Mutex::new(HashMap::new()),
            timers: Mutex::new(HashMap::new()),
            session: watch::Sender::new(LOCAL_SESSION),
            commits: broadcast::channel(64).0,
        })))
    }

    /// Reads every actor file's row. Starts each actor whose alarm
    /// is due or whose run a kill cut off, and arms a timer for each
    /// later alarm. Returns the started actors. An actor that fails
    /// to start is logged and left for the next boot.
    pub async fn boot(&self) -> Result<Vec<String>> {
        let mut started = Vec::new();
        for stored in self.stored_actors()? {
            let actor_id = stored.actor_id;
            let due =
                stored.wake_at.filter(|&wake_at| wake_at <= now_ms());
            if let Some(wake_at) = stored.wake_at {
                self.0
                    .alarms
                    .lock()
                    .insert(actor_id.clone(), wake_at);
                if due.is_none() {
                    arm_alarm(&self.0, actor_id.clone(), wake_at);
                }
            }
            let start = match due {
                Some(wake_at) => {
                    self.fire_alarm(&actor_id, wake_at).await
                }
                None if stored.cut_off => {
                    self.ensure_started(&actor_id).await
                }
                None => continue,
            };
            match start {
                Ok(()) => started.push(actor_id),
                Err(error) => tracing::error!(
                    ?error,
                    actor_id,
                    "host failed to start an actor at boot"
                ),
            }
        }
        Ok(started)
    }

    /// The actor for `kind` and `key`, started if it is not running.
    /// Returns its id.
    pub async fn get_or_start(
        &self,
        kind: &str,
        key: &str,
    ) -> Result<String> {
        let actor_id = actor_id(kind, key)?;
        self.ensure_started(&actor_id).await?;
        Ok(actor_id)
    }

    /// Calls an action on a running actor. A call to an actor that is
    /// starting waits for its start.
    pub async fn action(
        &self,
        actor_id: &str,
        name: &str,
        args: Vec<u8>,
    ) -> Result<Vec<u8>> {
        let generation = loop {
            let mut ready = match self.0.actors.lock().get(actor_id) {
                Some(Life::Running(generation)) => {
                    break generation.clone();
                }
                Some(Life::Starting { ready, .. }) => ready.clone(),
                _ => bail!("actor {actor_id} is not running"),
            };
            let _ = ready.changed().await;
        };
        generation.actor.action(name, args).await
    }

    /// Whether the actor holds its file: from the end of its start
    /// until its stop closed the file.
    pub fn is_running(&self, actor_id: &str) -> bool {
        self.0
            .actors
            .lock()
            .get(actor_id)
            .is_some_and(|life| life.loaded().is_some())
    }

    /// Follows the actors whose transactions commit, by id.
    pub fn commits(&self) -> broadcast::Receiver<String> {
        self.0.commits.subscribe()
    }

    /// Stops the actor and closes its file, or removes it for a
    /// destroy. A start under way is waited for, then stopped. A stop
    /// under way is waited for, and a destroy makes it a destroy.
    pub async fn stop(
        &self,
        actor_id: &str,
        reason: ShutdownKind,
    ) -> Result<()> {
        loop {
            if let Some(stop) =
                self.0.ask_stop(actor_id, None, reason)
            {
                return tokio::spawn(stop.run()).await?;
            }
            let (mut waiters, starting) =
                match self.0.actors.lock().get(actor_id) {
                    None => return Ok(()),
                    // It started since: ask again.
                    Some(Life::Running(_)) => continue,
                    Some(Life::Starting { ready, .. }) => {
                        (ready.clone(), true)
                    }
                    Some(
                        Life::Stopping { done, .. }
                        | Life::Destroying { done, .. },
                    ) => (done.clone(), false),
                };
            let _ = waiters.changed().await;
            if !starting {
                return Ok(());
            }
        }
    }

    pub async fn stop_all(&self) -> Result<()> {
        let ids: Vec<String> =
            self.0.actors.lock().keys().cloned().collect();
        for id in ids {
            self.stop(&id, ShutdownKind::Sleep).await?;
        }
        Ok(())
    }

    /// Starts the actor unless it runs. Waits for a start or a stop
    /// under way first.
    async fn ensure_started(&self, actor_id: &str) -> Result<()> {
        enum Next {
            Wait(watch::Receiver<()>),
            Start(Owned),
        }
        loop {
            let next = {
                let mut actors = self.0.actors.lock();
                match actors.get(actor_id) {
                    Some(Life::Running(_)) => return Ok(()),
                    Some(Life::Destroying { .. }) => {
                        bail!("actor {actor_id} is being destroyed")
                    }
                    Some(
                        Life::Starting { ready: waiters, .. }
                        | Life::Stopping { done: waiters, .. },
                    ) => Next::Wait(waiters.clone()),
                    None => {
                        let owned = Owned::new(&self.0, actor_id);
                        actors.insert(
                            actor_id.to_owned(),
                            Life::Starting {
                                generation: None,
                                request: None,
                                ready: owned.waiters.clone(),
                            },
                        );
                        Next::Start(owned)
                    }
                }
            };
            match next {
                Next::Wait(mut waiters) => {
                    let _ = waiters.changed().await;
                }
                Next::Start(owned) => {
                    return tokio::spawn(start_actor(owned)).await?;
                }
            }
        }
    }

    /// Wakes the actor for its alarm. The alarm is one-shot, so
    /// firing consumes it once the start, which drains overdue
    /// schedules, has returned; an alarm the start set in its place
    /// is kept.
    async fn fire_alarm(
        &self,
        actor_id: &str,
        wake_at: i64,
    ) -> Result<()> {
        self.ensure_started(actor_id).await?;
        {
            let mut alarms = self.0.alarms.lock();
            if alarms.get(actor_id) != Some(&wake_at) {
                return Ok(());
            }
            alarms.remove(actor_id);
        }
        let file = self
            .0
            .actors
            .lock()
            .get(actor_id)
            .and_then(Life::loaded)
            .map(|generation| generation.file.clone());
        let Some(file) = file else {
            return Ok(());
        };
        file.host_write(
            "alarm-fired",
            "UPDATE _hamlet_actor SET wake_at = NULL WHERE wake_at \
             = ?",
            vec![Value::Integer(wake_at)],
        )?
        .wait()
        .await
    }

    /// Each actor file's row that holds an alarm or a run a kill cut
    /// off. The files are closed again; starting an actor opens its
    /// file anew.
    fn stored_actors(&self) -> Result<Vec<StoredActor>> {
        let mut stored = Vec::new();
        let actors = self.0.dir.join("actors");
        for kind in std::fs::read_dir(&actors)? {
            let kind = kind?;
            if !kind.file_type()?.is_dir() {
                continue;
            }
            for file in std::fs::read_dir(kind.path())? {
                let path = file?.path();
                if path.extension().is_none_or(|ext| ext != "sqlite")
                {
                    continue;
                }
                if let Some(actor) = stored_actor(&path)
                    .with_context(|| {
                        format!("read {}", path.display())
                    })?
                {
                    stored.push(actor);
                }
            }
        }
        Ok(stored)
    }
}

/// The part of one actor file's `_hamlet_actor` row that boot reads.
struct StoredActor {
    actor_id: String,
    wake_at: Option<i64>,
    /// The actor was running when the host died: no clean stop
    /// cleared its mark.
    cut_off: bool,
}

/// The `_hamlet_actor` row of one actor file, if it holds an alarm
/// or a run a kill cut off.
fn stored_actor(path: &Path) -> Result<Option<StoredActor>> {
    let conn = plain_sqlite::open(path)?;
    let has_row: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_schema WHERE name = \
         '_hamlet_actor')",
        [],
        |row| row.get(0),
    )?;
    if !has_row {
        return Ok(None);
    }
    let stored: Option<(String, String, Option<i64>, bool)> = conn
        .query_row(
            "SELECT kind, key, wake_at, running FROM _hamlet_actor",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                ))
            },
        )
        .optional()?;
    match stored {
        Some((kind, key, wake_at, cut_off))
            if wake_at.is_some() || cut_off =>
        {
            Ok(Some(StoredActor {
                actor_id: actor_id(&kind, &key)?,
                wake_at,
                cut_off,
            }))
        }
        _ => Ok(None),
    }
}

/// When the alarm is due and still stored, wakes the actor. A running
/// actor's own timer fires the alarm; this only starts a stopped one.
/// The timer replaces the actor's earlier one, unless that one is
/// already due. An alarm that is no longer stored arms nothing: each
/// store arms from a task of its own, so arms can come out of order.
fn arm_alarm(host: &Arc<HostInner>, actor_id: String, wake_at: i64) {
    let weak = Arc::downgrade(host);
    // Held until the new timer is in the map, so the timer cannot
    // look for itself there first.
    let mut timers = host.timers.lock();
    if host.alarms.lock().get(&actor_id) != Some(&wake_at) {
        return;
    }
    let timer = tokio::spawn({
        let actor_id = actor_id.clone();
        async move {
            let delay =
                u64::try_from(wake_at - now_ms()).unwrap_or(0);
            tokio::time::sleep(std::time::Duration::from_millis(
                delay,
            ))
            .await;
            let Some(host) = weak.upgrade() else { return };
            // A due timer leaves the map, so an alarm its own start
            // sets does not abort it while it fires.
            {
                let mut timers = host.timers.lock();
                if timers.get(&actor_id).is_some_and(|timer| {
                    timer.id() == tokio::task::id()
                }) {
                    timers.remove(&actor_id);
                }
            }
            if host.alarms.lock().get(&actor_id) != Some(&wake_at) {
                return;
            }
            if let Err(error) =
                Host(host).fire_alarm(&actor_id, wake_at).await
            {
                tracing::error!(
                    ?error,
                    actor_id,
                    "host alarm failed to start actor"
                );
            }
        }
    });
    if let Some(earlier) =
        timers.insert(actor_id, timer.abort_handle())
    {
        earlier.abort();
    }
}

/// The id of the actor of `kind` and `key`, which names its file.
/// Both must be plain file names.
fn actor_id(kind: &str, key: &str) -> Result<String> {
    for part in [kind, key] {
        let plain = !part.is_empty()
            && !part.starts_with('.')
            && part.len() <= 128
            && part.bytes().all(|byte| {
                byte.is_ascii_alphanumeric()
                    || matches!(byte, b'-' | b'_' | b'.')
            });
        if !plain {
            bail!(
                "`{part}` is not a valid actor kind or key: use \
                 letters, digits, `-`, `_` and `.`, and do not \
                 start with `.`"
            );
        }
    }
    Ok(format!("{kind}/{key}"))
}

fn actor_path(dir: &Path, actor_id: &str) -> PathBuf {
    dir.join("actors").join(format!("{actor_id}.sqlite"))
}

/// Opens the actor's file, adds a generation to its `_hamlet_actor`
/// row, marks it running, and serves it. Returns the file, the new
/// generation, and whether this is the actor's first start.
fn open_actor_file(
    dir: &Path,
    kind: &str,
    key: &str,
) -> Result<(PlainSqlite, u32, bool)> {
    std::fs::create_dir_all(dir.join("actors").join(kind))?;
    let path = actor_path(dir, &format!("{kind}/{key}"));
    let mut conn = plain_sqlite::open(&path)?;
    let (generation, first) =
        bump_generation(&mut conn, kind, key)
            .with_context(|| format!("start {}", path.display()))?;
    Ok((PlainSqlite::serve(conn)?, generation, first))
}

fn bump_generation(
    conn: &mut Connection,
    kind: &str,
    key: &str,
) -> Result<(u32, bool)> {
    let tx = conn.transaction()?;
    tx.execute_batch(ACTOR_TABLE)?;
    let first = tx.execute(
        "INSERT OR IGNORE INTO _hamlet_actor (id, kind, key, \
         generation) VALUES (0, ?, ?, 0)",
        (kind, key),
    )? == 1;
    let generation = tx.query_row(
        "UPDATE _hamlet_actor SET generation = generation + 1, \
         running = 1 RETURNING generation",
        [],
        |row| row.get(0),
    )?;
    tx.commit()?;
    Ok((generation, first))
}

/// Starts the actor that `owned` holds as `Starting`: opens its file
/// off the async runtime, starts its core, and makes it `Running`, or
/// begins the stop its generation asked for meanwhile.
async fn start_actor(owned: Owned) -> Result<()> {
    let host = owned.host.clone();
    let actor_id = owned.actor_id.clone();
    let (kind, key) = actor_id
        .split_once('/')
        .ok_or_else(|| anyhow!("actor id {actor_id} has no kind"))?;
    let factory =
        host.factories.get(kind).cloned().ok_or_else(|| {
            anyhow!("actor kind `{kind}` is not registered")
        })?;
    let (file, number, first) = tokio::task::spawn_blocking({
        let dir = host.dir.clone();
        let (kind, key) = (kind.to_owned(), key.to_owned());
        move || open_actor_file(&dir, &kind, &key)
    })
    .await??;
    if let Some(Life::Starting { generation, .. }) =
        host.actors.lock().get_mut(&actor_id)
    {
        *generation = Some(number);
    }
    let started = RunningActor::start(StartRequest {
        actor_id: actor_id.clone(),
        name: kind.to_owned(),
        key: vec![ActorKeySegment::String(key.to_owned())],
        generation: number,
        factory,
        handle: Arc::new(LocalHandle {
            host: Arc::downgrade(&host),
            actor_id: actor_id.clone(),
            generation: number,
            file: file.clone(),
        }),
        new_storage: first,
        input: None,
    })
    .await;
    let actor = match started {
        Ok(actor) => actor,
        // A failed start keeps its running mark, so the next boot
        // tries it again rather than leave an alarm it may not have
        // stored.
        Err(error) => {
            file.close().await;
            return Err(error);
        }
    };
    let generation = Arc::new(Generation {
        number,
        actor,
        file,
    });
    let stop = {
        let mut actors = host.actors.lock();
        let Some(Life::Starting { request, .. }) =
            actors.get(&actor_id)
        else {
            unreachable!(
                "only its start moves an actor out of Starting"
            )
        };
        match *request {
            None => {
                actors.insert(
                    actor_id,
                    Life::Running(generation.clone()),
                );
                None
            }
            Some(reason) => {
                let owned = Owned::new(&host, &actor_id);
                actors.insert(
                    actor_id,
                    stopping(&owned, generation.clone(), reason),
                );
                Some(Stop {
                    owned,
                    generation,
                    reason,
                })
            }
        }
    };
    // Wakes the start's waiters, which find the actor in its new
    // state.
    drop(owned);
    if let Some(stop) = stop {
        stop.spawn_logged();
    }
    Ok(())
}

impl LocalHandle {
    /// This generation asks to stop. A request from a generation that
    /// has ended does nothing.
    fn ask_stop(&self, reason: ShutdownKind) {
        if let Some(stop) = self.host.upgrade().and_then(|host| {
            host.ask_stop(
                &self.actor_id,
                Some(self.generation),
                reason,
            )
        }) {
            stop.spawn_logged();
        }
    }
}

impl ActorHost for LocalHandle {
    fn endpoint(&self) -> &str {
        "local"
    }

    fn namespace(&self) -> &str {
        "local"
    }

    fn pool_name(&self) -> &str {
        "local"
    }

    fn connection_session(&self) -> Option<u64> {
        Some(LOCAL_SESSION)
    }

    /// The session never changes, so a transaction never sees its
    /// connection lost.
    fn subscribe_connection_session(&self) -> watch::Receiver<u64> {
        match self.host.upgrade() {
            Some(host) => host.session.subscribe(),
            None => watch::channel(0).1,
        }
    }

    /// A crash, reported with an error, stops the actor as a sleep.
    fn sleep_actor(
        &self,
        _actor_id: String,
        _generation: Option<u32>,
        _error: Option<String>,
    ) {
        self.ask_stop(ShutdownKind::Sleep);
    }

    fn stop_actor(
        &self,
        _actor_id: String,
        _generation: Option<u32>,
    ) {
        self.ask_stop(ShutdownKind::Destroy);
    }

    /// Writes the alarm to the actor's file once the actor has no
    /// transaction open, then acknowledges it. An alarm from a
    /// generation that has ended is acknowledged and not stored.
    fn set_alarm_with_ack(
        &self,
        _actor_id: String,
        alarm_ts: Option<i64>,
        _generation: Option<u32>,
        ack_tx: Option<oneshot::Sender<()>>,
    ) {
        let Some(host) = self.host.upgrade() else {
            return;
        };
        let actor_id = self.actor_id.clone();
        {
            let actors = host.actors.lock();
            if !actors.get(&actor_id).is_some_and(|life| {
                life.is_generation(self.generation)
            }) {
                drop(actors);
                if let Some(ack_tx) = ack_tx {
                    let _ = ack_tx.send(());
                }
                return;
            }
            match alarm_ts {
                Some(wake_at) => host
                    .alarms
                    .lock()
                    .insert(actor_id.clone(), wake_at),
                None => host.alarms.lock().remove(&actor_id),
            };
        }
        // Queued here, so the file stores alarms in the order the
        // core set them, and before a stop closes it.
        let stored = self.file.host_write(
            "alarm-store",
            "UPDATE _hamlet_actor SET wake_at = ?",
            vec![alarm_ts.map_or(Value::Null, Value::Integer)],
        );
        let host = Arc::downgrade(&host);
        tokio::spawn(async move {
            if let Err(error) = async { stored?.wait().await }.await {
                tracing::error!(
                    ?error,
                    actor_id,
                    "host failed to store alarm"
                );
                return;
            }
            if let (Some(wake_at), Some(host)) =
                (alarm_ts, host.upgrade())
            {
                arm_alarm(&host, actor_id, wake_at);
            }
            if let Some(ack_tx) = ack_tx {
                let _ = ack_tx.send(());
            }
        });
    }

    fn sqlite_exec(
        &self,
        _actor_id: String,
        sql: String,
    ) -> RuntimeBoxFuture<Result<QueryResult>> {
        let file = self.file.clone();
        Box::pin(async move { file.exec(sql).await })
    }

    fn sqlite_execute(
        &self,
        _actor_id: String,
        sql: String,
        params: Option<Vec<BindParam>>,
    ) -> RuntimeBoxFuture<Result<ExecuteResult>> {
        let file = self.file.clone();
        Box::pin(async move { file.execute(sql, params).await })
    }

    fn sqlite_execute_batch(
        &self,
        _actor_id: String,
        statements: Vec<SqliteBatchStatement>,
    ) -> RuntimeBoxFuture<Result<Vec<ExecuteResult>>> {
        let file = self.file.clone();
        Box::pin(async move { file.execute_batch(statements).await })
    }

    fn transaction_committed(&self, actor_id: &str) {
        if let Some(host) = self.host.upgrade() {
            let _ = host.commits.send(actor_id.to_owned());
        }
    }
}

/// Milliseconds since the Unix epoch, the clock alarms use.
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "../examples/demo/counter.rs"]
mod counter;

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use hamlet_testing::Scratch;
    use tokio::time::{Instant, sleep};

    use super::*;

    /// Each store arms its alarm from a task of its own, so an
    /// earlier alarm's arm can come after a newer one's. It must not
    /// replace the newer timer.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_late_arm_of_a_replaced_alarm_keeps_the_newer_timer() {
        let dir = Scratch::new("host-late-arm");
        let host = Host::open(
            &dir,
            counter::factories(counter::Sleep::WhenIdle),
        )
        .unwrap();
        let id = host.get_or_start("counter", "a").await.unwrap();
        host.action(&id, "schedule", 300u64.to_le_bytes().to_vec())
            .await
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !host.0.timers.lock().contains_key(&id) {
            assert!(
                Instant::now() < deadline,
                "the alarm was not armed"
            );
            sleep(Duration::from_millis(10)).await;
        }
        host.stop(&id, ShutdownKind::Sleep).await.unwrap();

        // The arm of an alarm the 300 ms one replaced, arriving last.
        arm_alarm(&host.0, id.clone(), now_ms() + 600_000);

        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if host.is_running(&id) {
                let reply = host
                    .action(&id, "get", Vec::new())
                    .await
                    .unwrap();
                if counter::count(reply).unwrap() == 1 {
                    break;
                }
            }
            assert!(
                Instant::now() < deadline,
                "the actor never woke"
            );
            sleep(Duration::from_millis(20)).await;
        }
        host.stop_all().await.unwrap();
    }

    /// The core of an ended generation keeps its handle. Through it,
    /// that generation can neither stop nor destroy the actor's next
    /// generation, store an alarm for it, nor reach its file.
    #[tokio::test(flavor = "multi_thread")]
    async fn an_ended_generations_handle_reaches_nothing() {
        let dir = Scratch::new("host-ended-handle");
        let host = Host::open(
            &dir,
            counter::factories(counter::Sleep::WhenIdle),
        )
        .unwrap();
        let id = host.get_or_start("counter", "a").await.unwrap();
        let ended = {
            let actors = host.0.actors.lock();
            let generation = actors[&id].loaded().unwrap();
            LocalHandle {
                host: Arc::downgrade(&host.0),
                actor_id: id.clone(),
                generation: generation.number,
                file: generation.file.clone(),
            }
        };
        host.stop(&id, ShutdownKind::Sleep).await.unwrap();
        host.get_or_start("counter", "a").await.unwrap();
        host.action(&id, "increment", Vec::new()).await.unwrap();

        ended.sleep_actor(id.clone(), Some(1), None);
        ended.sleep_actor(id.clone(), Some(1), Some("crash".into()));
        ended.stop_actor(id.clone(), Some(1));
        let (ack, acked) = oneshot::channel();
        ended.set_alarm_with_ack(
            id.clone(),
            Some(now_ms() + 60_000),
            Some(1),
            Some(ack),
        );
        acked
            .await
            .expect("the ended generation's alarm was not acked");
        assert!(
            ended
                .sqlite_execute(
                    id.clone(),
                    "UPDATE counter SET value = 100".to_owned(),
                    None,
                )
                .await
                .is_err()
        );

        assert!(matches!(
            host.0.actors.lock().get(&id),
            Some(Life::Running(generation)) if generation.number == 2
        ));
        assert!(!host.0.alarms.lock().contains_key(&id));
        let reply =
            host.action(&id, "get", Vec::new()).await.unwrap();
        assert_eq!(counter::count(reply).unwrap(), 1);
        host.stop_all().await.unwrap();
    }
}
