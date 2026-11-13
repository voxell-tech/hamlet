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
use tokio::sync::{Mutex as AsyncMutex, broadcast, oneshot, watch};
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

struct HostInner {
    dir: PathBuf,
    /// Held open for the host's life: its lock keeps a second host
    /// out of the folder.
    _lock: File,
    factories: HashMap<String, Arc<ActorFactory>>,
    running: AsyncMutex<HashMap<String, Arc<RunningActor>>>,
    files: Mutex<HashMap<String, PlainSqlite>>,
    /// Each actor's next alarm, as last stored in its file.
    alarms: Mutex<HashMap<String, i64>>,
    /// Each actor's one armed alarm timer.
    timers: Mutex<HashMap<String, AbortHandle>>,
    session: watch::Sender<u64>,
    commits: broadcast::Sender<String>,
}

/// The actors of one realm folder, run in this process.
#[derive(Clone)]
pub struct Host(Arc<HostInner>);

/// The core's handle on the host.
struct LocalHandle(Weak<HostInner>);

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
            running: AsyncMutex::new(HashMap::new()),
            files: Mutex::new(HashMap::new()),
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

    /// Calls an action on a running actor.
    pub async fn action(
        &self,
        actor_id: &str,
        name: &str,
        args: Vec<u8>,
    ) -> Result<Vec<u8>> {
        let actor = self
            .0
            .running
            .lock()
            .await
            .get(actor_id)
            .cloned()
            .ok_or_else(|| {
                anyhow!("actor {actor_id} is not running")
            })?;
        actor.action(name, args).await
    }

    pub async fn is_running(&self, actor_id: &str) -> bool {
        self.0.running.lock().await.contains_key(actor_id)
    }

    /// Follows the actors whose transactions commit, by id.
    pub fn commits(&self) -> broadcast::Receiver<String> {
        self.0.commits.subscribe()
    }

    /// Stops the actor and closes its file.
    pub async fn stop(
        &self,
        actor_id: &str,
        reason: ShutdownKind,
    ) -> Result<()> {
        stop_actor(&self.0, actor_id, reason).await
    }

    pub async fn stop_all(&self) -> Result<()> {
        let ids: Vec<String> =
            self.0.running.lock().await.keys().cloned().collect();
        for id in ids {
            self.stop(&id, ShutdownKind::Sleep).await?;
        }
        Ok(())
    }

    async fn ensure_started(&self, actor_id: &str) -> Result<()> {
        let mut running = self.0.running.lock().await;
        if running.contains_key(actor_id) {
            return Ok(());
        }
        let actor = start_actor(&self.0, actor_id).await?;
        running.insert(actor_id.to_owned(), Arc::new(actor));
        Ok(())
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
        let Some(file) = self.0.files.lock().get(actor_id).cloned()
        else {
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
/// already due.
fn arm_alarm(host: &Arc<HostInner>, actor_id: String, wake_at: i64) {
    let weak = Arc::downgrade(host);
    // Held until the new timer is in the map, so the timer cannot
    // look for itself there first.
    let mut timers = host.timers.lock();
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

async fn start_actor(
    host: &Arc<HostInner>,
    actor_id: &str,
) -> Result<RunningActor> {
    let (kind, key) = actor_id
        .split_once('/')
        .ok_or_else(|| anyhow!("actor id {actor_id} has no kind"))?;
    let factory =
        host.factories.get(kind).cloned().ok_or_else(|| {
            anyhow!("actor kind `{kind}` is not registered")
        })?;
    let (file, generation, first) =
        open_actor_file(&host.dir, kind, key)?;
    host.files.lock().insert(actor_id.to_owned(), file.clone());
    let started = RunningActor::start(StartRequest {
        actor_id: actor_id.to_owned(),
        name: kind.to_owned(),
        key: vec![ActorKeySegment::String(key.to_owned())],
        generation,
        factory,
        handle: Arc::new(LocalHandle(Arc::downgrade(host))),
        new_storage: first,
        input: None,
    })
    .await;
    // A failed start keeps its running mark, so the next boot tries
    // it again rather than leave an alarm it may not have stored.
    if started.is_err() {
        host.files.lock().remove(actor_id);
        file.close().await;
    }
    started
}

async fn stop_actor(
    host: &Arc<HostInner>,
    actor_id: &str,
    reason: ShutdownKind,
) -> Result<()> {
    // The running list stays locked until the file is closed, or
    // removed for a destroy, so a start of the same actor waits for
    // the lock on its file to go.
    let mut running = host.running.lock().await;
    let Some(actor) = running.remove(actor_id) else {
        return Ok(());
    };
    actor.stop(reason).await;
    let file = host.files.lock().remove(actor_id);
    if let Some(file) = file {
        // Queued behind every alarm the actor stored, so the mark
        // goes only once they are in the file.
        let cleared = file.host_write(
            "run-ended",
            "UPDATE _hamlet_actor SET running = 0",
            Vec::new(),
        );
        file.close().await;
        if let Err(error) = async { cleared?.wait().await }.await {
            tracing::error!(
                ?error,
                actor_id,
                "host failed to clear the running mark"
            );
        }
    }
    if matches!(reason, ShutdownKind::Destroy) {
        host.alarms.lock().remove(actor_id);
        let path = actor_path(&host.dir, actor_id);
        std::fs::remove_file(&path)
            .with_context(|| format!("remove {}", path.display()))?;
    }
    drop(running);
    Ok(())
}

impl LocalHandle {
    fn sqlite_file(&self, actor_id: &str) -> Result<PlainSqlite> {
        let host = self
            .0
            .upgrade()
            .ok_or_else(|| anyhow!("host is gone"))?;
        let file = host.files.lock().get(actor_id).cloned();
        file.ok_or_else(|| {
            anyhow!("sqlite file for {actor_id} is not open")
        })
    }

    fn spawn_stop(&self, actor_id: String, reason: ShutdownKind) {
        let Some(host) = self.0.upgrade() else { return };
        tokio::spawn(async move {
            if let Err(error) =
                stop_actor(&host, &actor_id, reason).await
            {
                tracing::error!(
                    ?error,
                    actor_id,
                    "host failed to stop actor"
                );
            }
        });
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
        match self.0.upgrade() {
            Some(host) => host.session.subscribe(),
            None => watch::channel(0).1,
        }
    }

    fn sleep_actor(
        &self,
        actor_id: String,
        _generation: Option<u32>,
        _error: Option<String>,
    ) {
        self.spawn_stop(actor_id, ShutdownKind::Sleep);
    }

    fn stop_actor(&self, actor_id: String, _generation: Option<u32>) {
        self.spawn_stop(actor_id, ShutdownKind::Destroy);
    }

    /// Writes the alarm to the actor's file once the actor has no
    /// transaction open, then acknowledges it.
    fn set_alarm_with_ack(
        &self,
        actor_id: String,
        alarm_ts: Option<i64>,
        _generation: Option<u32>,
        ack_tx: Option<oneshot::Sender<()>>,
    ) {
        let Some(host) = self.0.upgrade() else { return };
        let file = match self.sqlite_file(&actor_id) {
            Ok(file) => file,
            Err(error) => {
                tracing::error!(
                    ?error,
                    actor_id,
                    "host failed to store alarm"
                );
                return;
            }
        };
        match alarm_ts {
            Some(wake_at) => {
                host.alarms.lock().insert(actor_id.clone(), wake_at)
            }
            None => host.alarms.lock().remove(&actor_id),
        };
        // Queued here, so the file stores alarms in the order the
        // core set them, and before a stop closes it.
        let stored = file.host_write(
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
        actor_id: String,
        sql: String,
    ) -> RuntimeBoxFuture<Result<QueryResult>> {
        let file = self.sqlite_file(&actor_id);
        Box::pin(async move { file?.exec(sql).await })
    }

    fn sqlite_execute(
        &self,
        actor_id: String,
        sql: String,
        params: Option<Vec<BindParam>>,
    ) -> RuntimeBoxFuture<Result<ExecuteResult>> {
        let file = self.sqlite_file(&actor_id);
        Box::pin(async move { file?.execute(sql, params).await })
    }

    fn sqlite_execute_batch(
        &self,
        actor_id: String,
        statements: Vec<SqliteBatchStatement>,
    ) -> RuntimeBoxFuture<Result<Vec<ExecuteResult>>> {
        let file = self.sqlite_file(&actor_id);
        Box::pin(async move { file?.execute_batch(statements).await })
    }

    fn transaction_committed(&self, actor_id: &str) {
        if let Some(host) = self.0.upgrade() {
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
