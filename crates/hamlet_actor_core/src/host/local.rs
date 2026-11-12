//! Runs actors in this process: finds an actor by name and key,
//! starts it, stops it, and keeps each actor's next alarm in an index
//! so an actor with a due alarm starts when the host boots.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};

use anyhow::{Context, Result, anyhow};
use parking_lot::Mutex;
use rusqlite::{Connection, OptionalExtension};
use tokio::sync::{Mutex as AsyncMutex, mpsc, oneshot};
use tokio::task::JoinHandle;

use super::plain_sqlite::PlainSqlite;
use super::tunnel::HibernatingWebSocketMetadata;
use crate::actor::kv::LegacyActorKv;
use crate::actor::task::{
    DispatchCommand, LifecycleCommand, try_send_dispatch_command,
    try_send_lifecycle_command,
};
use crate::async_counter::AsyncCounter;
use crate::telemetry::IncomingInvocationContext;
use crate::types::{ActorKey, ActorKeySegment, format_actor_key};
use crate::{
    ActorContext, ActorFactory, ActorTask, ShutdownKind, SqliteDb,
};

/// The one session a local file has. The transaction code compares
/// sessions to detect a lost connection, which a local file cannot
/// have.
const LOCAL_SESSION: u64 = 1;

struct Running {
    generation: u32,
    ctx: ActorContext,
    lifecycle: mpsc::UnboundedSender<LifecycleCommand>,
    dispatch: mpsc::UnboundedSender<DispatchCommand>,
    join: JoinHandle<Result<()>>,
}

struct HostInner {
    dir: PathBuf,
    factories: HashMap<String, Arc<ActorFactory>>,
    index: Mutex<Connection>,
    running: AsyncMutex<HashMap<String, Running>>,
    files: Mutex<HashMap<String, PlainSqlite>>,
    session: tokio::sync::watch::Sender<u64>,
}

#[derive(Clone)]
pub struct Host(Arc<HostInner>);

/// What the core calls to reach its host.
#[derive(Clone)]
pub struct HostHandle(Weak<HostInner>);

impl Host {
    pub fn open(
        dir: &Path,
        factories: HashMap<String, ActorFactory>,
    ) -> Result<Self> {
        std::fs::create_dir_all(dir.join("actors"))?;
        let index = Connection::open(dir.join("host.db"))?;
        index.execute_batch(
            "PRAGMA journal_mode = WAL; PRAGMA synchronous = FULL;
			CREATE TABLE IF NOT EXISTS actors (
				actor_id TEXT PRIMARY KEY, name TEXT NOT NULL, key TEXT NOT NULL,
				generation INTEGER NOT NULL, wake_at INTEGER);",
        )?;
        Ok(Self(Arc::new(HostInner {
            dir: dir.to_owned(),
            factories: factories
                .into_iter()
                .map(|(k, v)| (k, Arc::new(v)))
                .collect(),
            index: Mutex::new(index),
            running: AsyncMutex::new(HashMap::new()),
            files: Mutex::new(HashMap::new()),
            session: tokio::sync::watch::Sender::new(LOCAL_SESSION),
        })))
    }

    /// Starts every actor whose stored alarm is due, and arms a timer
    /// for each later one.
    pub async fn boot(&self) -> Result<Vec<String>> {
        let alarms: Vec<(String, i64)> = {
            let index = self.0.index.lock();
            let mut stmt = index.prepare(
                "SELECT actor_id, wake_at FROM actors WHERE wake_at \
                 IS NOT NULL",
            )?;
            stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?
        };
        let mut started = Vec::new();
        for (actor_id, wake_at) in alarms {
            if wake_at <= now_ms() {
                self.fire_alarm(&actor_id, wake_at).await?;
                started.push(actor_id);
            } else {
                self.arm_host_alarm(actor_id, wake_at);
            }
        }
        Ok(started)
    }

    /// The actor for `name` and `key`, started if it is not running.
    pub async fn get_or_start(
        &self,
        name: &str,
        key: &str,
    ) -> Result<String> {
        let actor_id = format!("{name}--{key}");
        {
            let index = self.0.index.lock();
            index.execute(
                "INSERT OR IGNORE INTO actors (actor_id, name, key, \
                 generation) VALUES (?, ?, ?, 0)",
                (&actor_id, name, key),
            )?;
        }
        self.ensure_started(&actor_id).await?;
        Ok(actor_id)
    }

    /// Calls an action: a connection for the call, the action through
    /// the actor task, then the connection closes.
    pub async fn action(
        &self,
        actor_id: &str,
        name: &str,
        args: Vec<u8>,
    ) -> Result<Vec<u8>> {
        let (ctx, dispatch) = {
            let running = self.0.running.lock().await;
            let actor = running.get(actor_id).ok_or_else(|| {
                anyhow!("actor {actor_id} is not running")
            })?;
            (actor.ctx.clone(), actor.dispatch.clone())
        };
        let conn = ctx
            .connect_conn_with_request(Vec::new(), None, async {
                Ok::<Vec<u8>, anyhow::Error>(Vec::new())
            })
            .await?;
        let (reply, rx) = oneshot::channel();
        try_send_dispatch_command(
            &dispatch,
            DispatchCommand::Action {
                name: name.to_owned(),
                args,
                incoming: IncomingInvocationContext::default(),
                conn: conn.clone(),
                reply,
            },
        )?;
        let result = rx
            .await
            .map_err(|_| anyhow!("actor dropped the action reply"))?;
        let _ = conn.disconnect(None).await;
        result
    }

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
        running.insert(actor_id.to_owned(), actor);
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
        self.0.index.lock().execute(
            "UPDATE actors SET wake_at = NULL WHERE actor_id = ? \
             AND wake_at = ?",
            (actor_id, wake_at),
        )?;
        Ok(())
    }

    fn arm_host_alarm(&self, actor_id: String, wake_at: i64) {
        arm_host_alarm(Arc::downgrade(&self.0), actor_id, wake_at);
    }
}

/// When the alarm is due and the actor is not running, starts it. A
/// running actor's own timer fires the alarm.
fn arm_host_alarm(
    host: Weak<HostInner>,
    actor_id: String,
    wake_at: i64,
) {
    tokio::spawn(async move {
        let delay = (wake_at - now_ms()).max(0) as u64;
        tokio::time::sleep(std::time::Duration::from_millis(delay))
            .await;
        let Some(host) = host.upgrade() else { return };
        let stored: Option<i64> = host
            .index
            .lock()
            .query_row(
                "SELECT wake_at FROM actors WHERE actor_id = ?",
                [&actor_id],
                |row| row.get(0),
            )
            .optional()
            .ok()
            .flatten()
            .flatten();
        if stored != Some(wake_at) {
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
    });
}

async fn start_actor(
    host: &Arc<HostInner>,
    actor_id: &str,
) -> Result<Running> {
    let (name, key, generation): (String, String, u32) = {
        let index = host.index.lock();
        index.execute(
            "UPDATE actors SET generation = generation + 1 WHERE \
             actor_id = ?",
            [actor_id],
        )?;
        index.query_row(
            "SELECT name, key, generation FROM actors WHERE \
             actor_id = ?",
            [actor_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?
    };
    let factory =
        host.factories.get(&name).cloned().ok_or_else(|| {
            anyhow!("actor `{name}` is not registered")
        })?;
    let path = host.dir.join("actors").join(format!("{actor_id}.db"));
    let new_actor_storage = !path.exists();
    host.files
        .lock()
        .insert(actor_id.to_owned(), PlainSqlite::open(&path)?);

    let handle = HostHandle(Arc::downgrade(host));
    let key: ActorKey = vec![ActorKeySegment::String(key)];
    let sql = SqliteDb::new_with_remote_sqlite(
        handle.clone(),
        actor_id.to_owned(),
        Some(format_actor_key(&key)),
        Some(generation as u64),
        factory.config().has_database,
        true,
    )?;
    let ctx = ActorContext::build(
        actor_id.to_owned(),
        name.clone(),
        key,
        "local".to_owned(),
        "local".to_owned(),
        Some(generation),
        "local".to_owned(),
        factory.config().clone(),
        LegacyActorKv::new(handle.clone(), actor_id.to_owned()),
        sql,
    );
    ctx.configure_envoy(handle, Some(generation));
    ctx.set_new_actor_storage(new_actor_storage);
    ctx.set_start_received_at(crate::time::Instant::now());

    let (lifecycle_tx, lifecycle_rx) = mpsc::unbounded_channel();
    let (dispatch_tx, dispatch_rx) = mpsc::unbounded_channel();
    let (events_tx, events_rx) = mpsc::unbounded_channel();
    ctx.configure_lifecycle_events(Some(events_tx));
    ctx.cancel_sleep_timer();
    ctx.set_local_alarm_callback(Some(Arc::new({
        let lifecycle_tx = lifecycle_tx.clone();
        move || {
            let lifecycle_tx = lifecycle_tx.clone();
            Box::pin(async move {
                let (reply, rx) = oneshot::channel();
                if try_send_lifecycle_command(
                    &lifecycle_tx,
                    LifecycleCommand::FireAlarm { reply },
                )
                .is_ok()
                {
                    let _ = rx.await;
                }
            })
        }
    })));
    let task = ActorTask::new(
        actor_id.to_owned(),
        generation,
        lifecycle_rx,
        dispatch_rx,
        events_rx,
        factory,
        ctx.clone(),
        None,
    );
    let join = tokio::spawn(task.run());
    let (reply, rx) = oneshot::channel();
    try_send_lifecycle_command(
        &lifecycle_tx,
        LifecycleCommand::Start { reply },
    )?;
    rx.await
        .context("receive actor task start reply")?
        .with_context(|| format!("start actor `{actor_id}`"))?;
    Ok(Running {
        generation,
        ctx,
        lifecycle: lifecycle_tx,
        dispatch: dispatch_tx,
        join,
    })
}

async fn stop_actor(
    host: &Arc<HostInner>,
    actor_id: &str,
    reason: ShutdownKind,
) -> Result<()> {
    let Some(actor) = host.running.lock().await.remove(actor_id)
    else {
        return Ok(());
    };
    let (reply, rx) = oneshot::channel();
    if try_send_lifecycle_command(
        &actor.lifecycle,
        LifecycleCommand::Stop { reason, reply },
    )
    .is_ok()
    {
        let _ = rx.await;
    }
    let _ = actor.join.await;
    host.files.lock().remove(actor_id);
    if matches!(reason, ShutdownKind::Destroy) {
        host.index.lock().execute(
            "DELETE FROM actors WHERE actor_id = ?",
            [actor_id],
        )?;
    }
    tracing::debug!(
        actor_id,
        generation = actor.generation,
        "actor stopped"
    );
    Ok(())
}

impl HostHandle {
    fn host(&self) -> Option<Arc<HostInner>> {
        self.0.upgrade()
    }

    pub(crate) fn sqlite_file(
        &self,
        actor_id: &str,
    ) -> Result<PlainSqlite> {
        let host =
            self.host().ok_or_else(|| anyhow!("host is gone"))?;
        let file = host.files.lock().get(actor_id).cloned();
        file.ok_or_else(|| {
            anyhow!("sqlite file for {actor_id} is not open")
        })
    }

    pub fn endpoint(&self) -> &str {
        "local"
    }

    pub fn token(&self) -> Option<&str> {
        None
    }

    pub fn namespace(&self) -> &str {
        "local"
    }

    pub fn pool_name(&self) -> &str {
        "local"
    }

    pub fn connection_session(&self) -> Option<u64> {
        Some(LOCAL_SESSION)
    }

    /// The session never changes, so a transaction never sees its
    /// connection lost.
    pub fn subscribe_connection_session(
        &self,
    ) -> tokio::sync::watch::Receiver<u64> {
        match self.host() {
            Some(host) => host.session.subscribe(),
            None => tokio::sync::watch::channel(0).1,
        }
    }

    /// The actor asks to sleep, and the host stops it.
    pub fn sleep_actor(
        &self,
        actor_id: String,
        _generation: Option<u32>,
        _error: Option<String>,
    ) {
        self.spawn_stop(actor_id, ShutdownKind::Sleep);
    }

    /// The actor asks to be destroyed.
    pub fn stop_actor(
        &self,
        actor_id: String,
        _generation: Option<u32>,
    ) {
        self.spawn_stop(actor_id, ShutdownKind::Destroy);
    }

    fn spawn_stop(&self, actor_id: String, reason: ShutdownKind) {
        let Some(host) = self.host() else { return };
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

    /// Stores the actor's next alarm durably, then acknowledges it.
    pub fn set_alarm_with_ack(
        &self,
        actor_id: String,
        alarm_ts: Option<i64>,
        _generation: Option<u32>,
        ack_tx: Option<oneshot::Sender<()>>,
    ) {
        let Some(host) = self.host() else { return };
        let stored = host.index.lock().execute(
            "UPDATE actors SET wake_at = ? WHERE actor_id = ?",
            (alarm_ts, &actor_id),
        );
        if let Err(error) = stored {
            tracing::error!(
                ?error,
                actor_id,
                "host failed to store alarm"
            );
            return;
        }
        if let Some(wake_at) = alarm_ts {
            arm_host_alarm(Arc::downgrade(&host), actor_id, wake_at);
        }
        if let Some(ack_tx) = ack_tx {
            let _ = ack_tx.send(());
        }
    }

    pub fn http_request_counter(
        &self,
        _actor_id: &str,
        _generation: Option<u32>,
    ) -> Option<Arc<AsyncCounter>> {
        None
    }

    pub fn hibernatable_connection_is_live(
        &self,
        _actor_id: &str,
        _generation: Option<u32>,
        _gateway_id: [u8; 4],
        _request_id: [u8; 4],
    ) -> bool {
        false
    }

    pub fn restore_hibernating_requests(
        &self,
        _actor_id: String,
        _entries: Vec<HibernatingWebSocketMetadata>,
    ) {
    }

    pub fn send_hibernatable_ws_message_ack(
        &self,
        _gateway_id: [u8; 4],
        _request_id: [u8; 4],
        _index: u16,
    ) {
    }

    /// The key-value store from before SQLite. A local host has none.
    pub async fn kv_get(
        &self,
        _actor_id: String,
        keys: Vec<Vec<u8>>,
    ) -> Result<Vec<Option<Vec<u8>>>> {
        Ok(vec![None; keys.len()])
    }

    pub async fn kv_put(
        &self,
        _actor_id: String,
        _entries: Vec<(Vec<u8>, Vec<u8>)>,
    ) -> Result<()> {
        Ok(())
    }

    pub async fn kv_list_prefix(
        &self,
        _actor_id: String,
        _prefix: Vec<u8>,
        _reverse: Option<bool>,
        _limit: Option<u64>,
    ) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        Ok(Vec::new())
    }

    pub async fn kv_list_range(
        &self,
        _actor_id: String,
        _start: Vec<u8>,
        _end: Vec<u8>,
        _exclusive: bool,
        _reverse: Option<bool>,
        _limit: Option<u64>,
    ) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        Ok(Vec::new())
    }
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

pub fn read_alarm(dir: &Path, actor_id: &str) -> Result<Option<i64>> {
    let index = Connection::open(dir.join("host.db"))?;
    Ok(index
        .query_row(
            "SELECT wake_at FROM actors WHERE actor_id = ?",
            [actor_id],
            |row| row.get(0),
        )
        .optional()?
        .flatten())
}
