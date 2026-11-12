// This file was changed for Hamlet. See NOTICE in this crate.
//! Test-only actor context fixtures backed by a real in-memory SQLite
//! database.
//!
//! The fixture keeps the production invariant that every
//! [`ActorContext`] has a configured SQLite backend. [`TestHost`]
//! stands where the engine handle stood: it serves the actor's SQL
//! from one in-memory connection on a thread of its own, and records
//! the alarms, sleeps and stops the core asks of its host.
//!
//! The core's tests are compiled into the library, so they cannot
//! link `hamlet_host`, which builds on a second copy of this crate.

use std::sync::{Arc, mpsc as std_mpsc};
use std::thread;

use anyhow::{Result, anyhow};
use hamlet_actor_wire::persist::versioned::RunWakeAt;
use parking_lot::Mutex;
use rusqlite::fallible_iterator::FallibleIterator;
use rusqlite::types::{Value, ValueRef};
use tokio::sync::{oneshot, watch};

use crate::ActorConfig;
use crate::actor::context::ActorContext;
use crate::actor::internal_storage::schema::initialize_test_schema;
use crate::actor::internal_storage::{self, RUN_WAKE_AT_META_KEY};
use crate::actor::kv::LegacyActorKv;
use crate::actor::persist::decode_latest_with_embedded_version;
use crate::actor::state::PersistedActor;
use crate::host::{ActorHost, HostHandle};
use crate::runtime::RuntimeBoxFuture;
use crate::sqlite::{
    BindParam, ColumnValue, ExecuteResult, QueryResult,
    SqliteBatchStatement, SqliteDb,
};
use crate::types::ActorKey;

/// Reusable in-memory SQLite store for constructing fully configured
/// contexts. Contexts created from the same harness observe the same
/// database.
#[derive(Clone)]
pub struct ActorContextHarness {
    host: Arc<TestHost>,
}

impl Default for ActorContextHarness {
    fn default() -> Self {
        Self::new()
    }
}

impl ActorContextHarness {
    pub fn new() -> Self {
        Self::with_client_endpoint("http://127.0.0.1:1")
    }

    /// Constructs a harness whose actor clients use the provided
    /// endpoint.
    pub fn with_client_endpoint(endpoint: impl Into<String>) -> Self {
        Self {
            host: Arc::new(TestHost::new(endpoint.into(), None)),
        }
    }

    /// A harness whose database stalls the statements `gate` matches.
    pub(crate) fn with_write_gate(gate: TestSqliteWriteGate) -> Self {
        Self {
            host: Arc::new(TestHost::new(
                "http://127.0.0.1:1".to_owned(),
                Some(gate),
            )),
        }
    }

    pub fn context(
        &self,
        actor_id: impl Into<String>,
        name: impl Into<String>,
        key: ActorKey,
        region: impl Into<String>,
    ) -> ActorContext {
        self.context_with_config(
            actor_id,
            name,
            key,
            region,
            ActorConfig::default(),
        )
    }

    pub fn context_with_config(
        &self,
        actor_id: impl Into<String>,
        name: impl Into<String>,
        key: ActorKey,
        region: impl Into<String>,
        config: ActorConfig,
    ) -> ActorContext {
        let actor_id = actor_id.into();
        let handle: HostHandle = self.host.clone();
        let kv = LegacyActorKv::new(handle, actor_id.clone());
        self.context_with_kv(actor_id, name, key, region, kv, config)
    }

    /// A context whose legacy KV is `kv`, such as the in-memory one.
    pub(crate) fn context_with_kv(
        &self,
        actor_id: impl Into<String>,
        name: impl Into<String>,
        key: ActorKey,
        region: impl Into<String>,
        kv: LegacyActorKv,
        config: ActorConfig,
    ) -> ActorContext {
        let actor_id = actor_id.into();
        let generation = Some(1);
        let handle: HostHandle = self.host.clone();
        let sql = SqliteDb::new_with_remote_sqlite(
            handle.clone(),
            actor_id.clone(),
            None,
            generation.map(u64::from),
            false,
            true,
        )
        .expect("test remote sqlite should be configured");
        let ctx = ActorContext::build(
            actor_id.clone(),
            name.into(),
            key,
            region.into(),
            handle.namespace().to_owned(),
            generation,
            handle.pool_name().to_owned(),
            config,
            kv,
            sql,
        );
        ctx.configure_envoy(handle, generation);
        ctx
    }
}

/// A context on its own database whose legacy KV is `kv`.
pub(crate) fn new_with_kv(
    actor_id: impl Into<String>,
    name: impl Into<String>,
    key: ActorKey,
    region: impl Into<String>,
    kv: LegacyActorKv,
) -> ActorContext {
    ActorContextHarness::new().context_with_kv(
        actor_id,
        name,
        key,
        region,
        kv,
        ActorConfig::default(),
    )
}

/// Like [`new_with_kv`], but the database stalls every statement
/// matching `gate` until the test releases it. Used to observe
/// in-flight state writes deterministically.
pub(crate) fn new_with_kv_and_write_gate(
    actor_id: impl Into<String>,
    name: impl Into<String>,
    key: ActorKey,
    region: impl Into<String>,
    kv: LegacyActorKv,
    gate: TestSqliteWriteGate,
) -> ActorContext {
    ActorContextHarness::with_write_gate(gate).context_with_kv(
        actor_id,
        name,
        key,
        region,
        kv,
        ActorConfig::default(),
    )
}

/// Constructs a context with its own in-memory SQLite store.
pub fn actor_context(
    actor_id: impl Into<String>,
    name: impl Into<String>,
    key: ActorKey,
    region: impl Into<String>,
) -> ActorContext {
    ActorContextHarness::new().context(actor_id, name, key, region)
}

/// Stalls the statements whose SQL starts with `sql_prefix`: each
/// matching request or batch first reports on `entered_tx`, then
/// waits for one permit on `release` before it runs.
pub(crate) struct TestSqliteWriteGate {
    pub(crate) sql_prefix: &'static str,
    pub(crate) entered_tx: tokio::sync::mpsc::UnboundedSender<()>,
    pub(crate) release: Arc<tokio::sync::Semaphore>,
}

impl TestSqliteWriteGate {
    fn pass(&self, mut sql: impl Iterator<Item = impl AsRef<str>>) {
        if sql.any(|sql| sql.as_ref().starts_with(self.sql_prefix)) {
            let _ = self.entered_tx.send(());
            futures::executor::block_on(self.release.acquire())
                .expect("write gate semaphore should stay open")
                .forget();
        }
    }
}

/// What the core asked of a [`TestHost`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostCall {
    SetAlarm {
        actor_id: String,
        alarm_ts: Option<i64>,
        generation: Option<u32>,
    },
    Sleep {
        actor_id: String,
        generation: Option<u32>,
        error: Option<String>,
    },
    Stop {
        actor_id: String,
        generation: Option<u32>,
    },
}

/// A host for tests: one in-memory SQLite database served on a thread
/// of its own, and a record of every alarm, sleep and stop.
pub struct TestHost {
    endpoint: String,
    sql: std_mpsc::Sender<SqlCommand>,
    session: watch::Sender<u64>,
    calls: Mutex<Vec<HostCall>>,
}

impl TestHost {
    fn new(
        endpoint: String,
        gate: Option<TestSqliteWriteGate>,
    ) -> Self {
        Self {
            endpoint,
            sql: spawn_sqlite(gate),
            session: watch::channel(1).0,
            calls: Mutex::new(Vec::new()),
        }
    }

    fn request<T: Send + 'static>(
        &self,
        command: impl FnOnce(oneshot::Sender<Result<T>>) -> SqlCommand,
    ) -> RuntimeBoxFuture<Result<T>> {
        let (reply, rx) = oneshot::channel();
        let sent = self.sql.send(command(reply));
        Box::pin(async move {
            sent.map_err(|_| anyhow!("test sqlite thread stopped"))?;
            rx.await
                .map_err(|_| anyhow!("test sqlite thread stopped"))?
        })
    }
}

impl ActorHost for TestHost {
    fn endpoint(&self) -> &str {
        &self.endpoint
    }

    fn namespace(&self) -> &str {
        "test"
    }

    fn pool_name(&self) -> &str {
        "test-envoy"
    }

    fn connection_session(&self) -> Option<u64> {
        Some(*self.session.borrow())
    }

    fn subscribe_connection_session(&self) -> watch::Receiver<u64> {
        self.session.subscribe()
    }

    fn sleep_actor(
        &self,
        actor_id: String,
        generation: Option<u32>,
        error: Option<String>,
    ) {
        self.calls.lock().push(HostCall::Sleep {
            actor_id,
            generation,
            error,
        });
    }

    fn stop_actor(&self, actor_id: String, generation: Option<u32>) {
        self.calls.lock().push(HostCall::Stop {
            actor_id,
            generation,
        });
    }

    fn set_alarm_with_ack(
        &self,
        actor_id: String,
        alarm_ts: Option<i64>,
        generation: Option<u32>,
        ack_tx: Option<oneshot::Sender<()>>,
    ) {
        self.calls.lock().push(HostCall::SetAlarm {
            actor_id,
            alarm_ts,
            generation,
        });
        if let Some(ack_tx) = ack_tx {
            let _ = ack_tx.send(());
        }
    }

    fn sqlite_exec(
        &self,
        _actor_id: String,
        sql: String,
    ) -> RuntimeBoxFuture<Result<QueryResult>> {
        self.request(|reply| SqlCommand::Exec(sql, reply))
    }

    fn sqlite_execute(
        &self,
        _actor_id: String,
        sql: String,
        params: Option<Vec<BindParam>>,
    ) -> RuntimeBoxFuture<Result<ExecuteResult>> {
        self.request(|reply| SqlCommand::Execute(sql, params, reply))
    }

    fn sqlite_execute_batch(
        &self,
        _actor_id: String,
        statements: Vec<SqliteBatchStatement>,
    ) -> RuntimeBoxFuture<Result<Vec<ExecuteResult>>> {
        self.request(|reply| SqlCommand::Batch(statements, reply))
    }

    fn transaction_committed(&self, _actor_id: &str) {}
}

enum SqlCommand {
    Exec(String, oneshot::Sender<Result<QueryResult>>),
    Execute(
        String,
        Option<Vec<BindParam>>,
        oneshot::Sender<Result<ExecuteResult>>,
    ),
    Batch(
        Vec<SqliteBatchStatement>,
        oneshot::Sender<Result<Vec<ExecuteResult>>>,
    ),
}

/// Serves one in-memory database, with the internal schema, on a
/// thread of its own until the host is dropped.
fn spawn_sqlite(
    gate: Option<TestSqliteWriteGate>,
) -> std_mpsc::Sender<SqlCommand> {
    let (tx, rx) = std_mpsc::channel::<SqlCommand>();
    thread::Builder::new()
        .name("test-sqlite".into())
        .spawn(move || {
            let conn = rusqlite::Connection::open_in_memory()
                .expect("test sqlite connection should open");
            initialize_test_schema(&conn)
                .expect("test sqlite schema should initialize");
            while let Ok(command) = rx.recv() {
                match command {
                    SqlCommand::Exec(sql, reply) => {
                        let _ = reply.send(exec_sqlite(&conn, &sql));
                    }
                    SqlCommand::Execute(sql, params, reply) => {
                        if let Some(gate) = &gate {
                            gate.pass(std::iter::once(&sql));
                        }
                        let _ = reply.send(execute_sqlite(
                            &conn,
                            &sql,
                            params.as_deref(),
                        ));
                    }
                    SqlCommand::Batch(statements, reply) => {
                        if let Some(gate) = &gate {
                            gate.pass(
                                statements.iter().map(|s| &s.sql),
                            );
                        }
                        let _ = reply.send(execute_sqlite_batch(
                            &conn, statements,
                        ));
                    }
                }
            }
        })
        .expect("test sqlite thread should start");
    tx
}

fn execute_sqlite_batch(
    conn: &rusqlite::Connection,
    statements: Vec<SqliteBatchStatement>,
) -> Result<Vec<ExecuteResult>> {
    conn.execute_batch("BEGIN")?;
    let mut results = Vec::with_capacity(statements.len());
    for statement in statements {
        match execute_sqlite(
            conn,
            &statement.sql,
            statement.params.as_deref(),
        ) {
            Ok(result) => results.push(result),
            Err(error) => {
                let _ = conn.execute_batch("ROLLBACK");
                return Err(error);
            }
        }
    }
    conn.execute_batch("COMMIT")?;
    Ok(results)
}

/// Runs each statement of `sql` and returns the rows of the last one
/// that has columns.
fn exec_sqlite(
    conn: &rusqlite::Connection,
    sql: &str,
) -> Result<QueryResult> {
    let mut result = QueryResult {
        columns: Vec::new(),
        rows: Vec::new(),
    };
    let mut batch = rusqlite::Batch::new(conn, sql);
    while let Some(mut statement) = batch.next()? {
        let (columns, rows) = read_rows(&mut statement)?;
        if !columns.is_empty() {
            result = QueryResult { columns, rows };
        }
    }
    Ok(result)
}

fn execute_sqlite(
    conn: &rusqlite::Connection,
    sql: &str,
    params: Option<&[BindParam]>,
) -> Result<ExecuteResult> {
    let mut statement = conn.prepare(sql)?;
    for (index, param) in
        params.unwrap_or_default().iter().enumerate()
    {
        statement.raw_bind_parameter(
            index + 1,
            sqlite_param_value(param),
        )?;
    }
    let (columns, rows) = read_rows(&mut statement)?;
    Ok(ExecuteResult {
        columns,
        rows,
        changes: conn.changes() as i64,
        last_insert_row_id: Some(conn.last_insert_rowid()),
    })
}

fn read_rows(
    statement: &mut rusqlite::Statement<'_>,
) -> Result<(Vec<String>, Vec<Vec<ColumnValue>>)> {
    let columns: Vec<String> = statement
        .column_names()
        .into_iter()
        .map(ToOwned::to_owned)
        .collect();
    let mut rows = Vec::new();
    let mut cursor = statement.raw_query();
    while let Some(row) = cursor.next()? {
        rows.push(
            (0..columns.len())
                .map(|index| {
                    Ok(sqlite_column_value(row.get_ref(index)?))
                })
                .collect::<Result<Vec<_>>>()?,
        );
    }
    Ok((columns, rows))
}

fn sqlite_param_value(param: &BindParam) -> Value {
    match param {
        BindParam::Null => Value::Null,
        BindParam::Integer(value) => Value::Integer(*value),
        BindParam::Float(value) => Value::Real(*value),
        BindParam::Text(value) => Value::Text(value.clone()),
        BindParam::Blob(value) => Value::Blob(value.clone()),
    }
}

fn sqlite_column_value(value: ValueRef<'_>) -> ColumnValue {
    match value {
        ValueRef::Null => ColumnValue::Null,
        ValueRef::Integer(value) => ColumnValue::Integer(value),
        ValueRef::Real(value) => ColumnValue::Float(value),
        ValueRef::Text(value) => ColumnValue::Text(
            String::from_utf8_lossy(value).into_owned(),
        ),
        ValueRef::Blob(value) => ColumnValue::Blob(value.to_vec()),
    }
}

/// The actor's last pushed alarm, read as the internal storage read
/// it when the tests were written.
pub(crate) async fn load_last_pushed_alarm(
    db: &SqliteDb,
) -> Result<Option<i64>> {
    let result = db
        .query(
            "SELECT last_pushed_alarm FROM _actor_runtime WHERE id \
             = 1",
            None,
        )
        .await?;
    match result.rows.first().and_then(|row| row.first()) {
        None | Some(ColumnValue::Null) => Ok(None),
        Some(ColumnValue::Integer(value)) => Ok(Some(*value)),
        Some(other) => Err(anyhow!("last_pushed_alarm is {other:?}")),
    }
}

/// The run handler's wake deadline, read as the internal storage read
/// it when the tests were written.
pub(crate) async fn load_run_wake_at(
    db: &SqliteDb,
) -> Result<Option<i64>> {
    let result = db
        .query(
            "SELECT value FROM _actor_meta WHERE key = ?",
            Some(vec![BindParam::Text(
                RUN_WAKE_AT_META_KEY.to_owned(),
            )]),
        )
        .await?;
    let Some(ColumnValue::Blob(payload)) =
        result.rows.first().and_then(|row| row.first())
    else {
        return Ok(None);
    };
    decode_latest_with_embedded_version::<RunWakeAt>(
        payload,
        "run wake deadline",
    )
}

/// Saves the actor's row and state, and nothing else.
pub(crate) async fn persist_actor_snapshot(
    db: &SqliteDb,
    actor: &PersistedActor,
) -> Result<()> {
    internal_storage::persist_actor_snapshot_with(
        db,
        actor,
        Vec::new(),
    )
    .await
}

// --- sleep ---

impl TestHost {
    /// A host of its own, for a test that hands it to a context.
    pub(crate) fn new_for_tests() -> Arc<Self> {
        Arc::new(Self::new("http://127.0.0.1:1".to_owned(), None))
    }

    /// Waits for the oldest call not yet taken and takes it. The
    /// calls are recorded synchronously, so this polls; under paused
    /// time each poll advances the clock by a millisecond.
    pub(crate) async fn next_call(&self) -> HostCall {
        loop {
            {
                let mut calls = self.calls.lock();
                if !calls.is_empty() {
                    return calls.remove(0);
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(1))
                .await;
        }
    }
}

// --- migq ---

/// Creates or migrates the internal schema, as the reference's
/// `ensure_internal_schema` did before upstream split it into a probe
/// and an ensure.
pub(crate) async fn ensure_internal_schema(
    db: &SqliteDb,
) -> Result<()> {
    let has_schema =
        internal_storage::schema::has_internal_schema(db).await?;
    internal_storage::schema::ensure_probed_internal_schema(
        db, has_schema,
    )
    .await
}

// --- ctxstate ---

/// The actor's saved row, alarm and run wake deadline, as the
/// internal storage loaded them when the tests were written.
pub(crate) async fn load_actor_snapshot(
    db: &SqliteDb,
) -> Result<Option<internal_storage::InternalActorSnapshot>> {
    Ok(internal_storage::load_actor_startup(db).await?.snapshot)
}
