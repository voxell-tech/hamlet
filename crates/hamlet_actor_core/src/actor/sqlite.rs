// This file was changed for Hamlet. See NOTICE in this crate.
use std::collections::HashSet;
use std::future::Future;
use std::io::Cursor;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Context, Result};
use deferred_bootstrap::DeferredBootstrap;
use hamlet_actor_error::{ActorError, ActorSpecifier};
pub use hamlet_actor_wire::sqlite_types::{
    BindParam, ColumnValue, ExecResult, ExecuteResult, QueryResult,
};
use serde::Serialize;
use serde_json::{Map as JsonMap, Value as JsonValue};
use tokio::sync::watch;
use tracing::Instrument as _;

use crate::host::HostHandle as EnvoyHandle;
use crate::host::utils::{
    RemoteSqliteConnectionSessionLostError,
    RemoteSqliteIndeterminateResultError,
};
use crate::telemetry::SqliteOperation;

mod deferred_bootstrap;
mod tx;

pub use tx::{
    DEFAULT_TRANSACTION_TIMEOUT, ManualTransactionOpenError,
    SqliteTransaction, TRANSACTION_COORDINATOR_QUEUE_CAPACITY,
    TransactionConnectionLostError,
    TransactionCoordinatorClosedError, TransactionExpiredError,
    TransactionInvalidArgumentError, TransactionQueueFullError,
    TransactionTerminalError, TransactionUnknownError,
};
#[cfg(test)]
use tx::{
    TRANSACTION_TERMINAL_CAPACITY, TransactionCoordinatorState,
    TransactionTerminalState, insert_terminal_state,
};
use tx::{TransactionCoordinator, run_detached_transaction_task};

use crate::error::SqliteRuntimeError;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SqliteVfsMetricsSnapshot {
    pub request_build_ns: u64,
    pub serialize_ns: u64,
    pub transport_ns: u64,
    pub state_update_ns: u64,
    pub total_ns: u64,
    pub commit_count: u64,
}

#[derive(Clone)]
pub struct SqliteRuntimeConfig {
    pub handle: EnvoyHandle,
    pub actor_id: String,
    pub generation: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct SqliteBatchStatement {
    pub sql: String,
    pub params: Option<Vec<BindParam>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SqliteBackend {
    LocalNative,
    RemoteEnvoy,
}

/// How [`SqliteDb::close_backend`] closes a `LocalNative` worker. A
/// remote database has no worker, so it ignores this.
#[derive(Clone, Copy, Debug)]
enum NativeClose {
    /// Waits up to the worker's close timeout. A running command
    /// finishes first.
    Bounded,
    /// Waits until the worker relinquished SQLite. A running command
    /// finishes first.
    Strict,
    /// Like `Strict`, but first interrupts the running command, for a
    /// teardown that has no use for its outcome, such as a failed
    /// start's.
    StrictInterrupting,
}

impl SqliteDb {
    async fn exec_backend(&self, sql: String) -> Result<QueryResult> {
        match self.backend {
            SqliteBackend::LocalNative => self.local_exec(sql).await,
            SqliteBackend::RemoteEnvoy => self.remote_exec(sql).await,
        }
    }

    async fn exec_backend_in_session(
        &self,
        sql: String,
        expected_session: Option<u64>,
    ) -> Result<(QueryResult, Option<u64>)> {
        match self.backend {
            SqliteBackend::LocalNative => self
                .local_exec(sql)
                .await
                .map(|result| (result, None)),
            SqliteBackend::RemoteEnvoy => self
                .remote_exec_with_session(sql, expected_session)
                .await
                .map(|(result, session)| (result, Some(session))),
        }
    }

    async fn query_backend(
        &self,
        sql: String,
        params: Option<Vec<BindParam>>,
    ) -> Result<QueryResult> {
        match self.backend {
            SqliteBackend::LocalNative => self
                .local_execute(sql, params)
                .await
                .map(ExecuteResult::into_query_result),
            SqliteBackend::RemoteEnvoy => self
                .remote_execute(sql, params)
                .await
                .map(ExecuteResult::into_query_result),
        }
    }

    async fn run_backend(
        &self,
        sql: String,
        params: Option<Vec<BindParam>>,
    ) -> Result<ExecResult> {
        match self.backend {
            SqliteBackend::LocalNative => self
                .local_execute(sql, params)
                .await
                .map(ExecuteResult::into_exec_result),
            SqliteBackend::RemoteEnvoy => self
                .remote_execute(sql, params)
                .await
                .map(ExecuteResult::into_exec_result),
        }
    }

    async fn execute_backend(
        &self,
        sql: String,
        params: Option<Vec<BindParam>>,
    ) -> Result<ExecuteResult> {
        match self.backend {
            SqliteBackend::LocalNative => {
                self.local_execute(sql, params).await
            }
            SqliteBackend::RemoteEnvoy => {
                self.remote_execute(sql, params).await
            }
        }
    }

    async fn execute_backend_in_session(
        &self,
        sql: String,
        params: Option<Vec<BindParam>>,
        expected_session: Option<u64>,
    ) -> Result<(ExecuteResult, Option<u64>)> {
        match self.backend {
            SqliteBackend::LocalNative => self
                .local_execute(sql, params)
                .await
                .map(|result| (result, None)),
            SqliteBackend::RemoteEnvoy => self
                .remote_execute_with_session(
                    sql,
                    params,
                    expected_session,
                )
                .await
                .map(|(result, session)| (result, Some(session))),
        }
    }
}

#[derive(Clone)]
pub struct SqliteDb {
    handle: Option<EnvoyHandle>,
    actor_id: Option<String>,
    actor_key: Option<String>,
    generation: Option<u64>,
    backend: SqliteBackend,
    /// Mirrors the user's actor-config `db({...})` declaration. The
    /// envoy always sets up sqlite storage under the hood, so
    /// handle/actor_id are not a reliable signal for whether the
    /// user opted in; this flag is.
    enabled: bool,
    invocation_telemetry: Option<crate::ActorInvocationTelemetry>,
    worker_fatal_reported: Arc<AtomicBool>,
    transaction_coordinator: Arc<TransactionCoordinator>,
    deferred_bootstrap: Arc<DeferredBootstrap>,
}

impl Default for SqliteDb {
    fn default() -> Self {
        Self {
            handle: None,
            actor_id: None,
            actor_key: None,
            generation: None,
            backend: SqliteBackend::RemoteEnvoy,
            enabled: false,
            invocation_telemetry: None,
            worker_fatal_reported: Default::default(),
            transaction_coordinator: Default::default(),
            deferred_bootstrap: Default::default(),
        }
    }
}

impl SqliteDb {
    pub fn new(
        handle: EnvoyHandle,
        actor_id: impl Into<String>,
        enabled: bool,
    ) -> Result<Self> {
        Self::new_with_remote_sqlite(
            handle, actor_id, None, None, enabled, false,
        )
    }

    #[expect(
        clippy::fn_params_excessive_bools,
        reason = "ported signature"
    )]
    pub fn new_with_remote_sqlite(
        handle: EnvoyHandle,
        actor_id: impl Into<String>,
        actor_key: Option<String>,
        generation: Option<u64>,
        enabled: bool,
        remote_sqlite: bool,
    ) -> Result<Self> {
        Ok(Self {
            handle: Some(handle),
            actor_id: Some(actor_id.into()),
            actor_key,
            generation,
            backend: select_sqlite_backend(remote_sqlite)?,
            enabled,
            invocation_telemetry: None,
            worker_fatal_reported: Default::default(),
            transaction_coordinator: Default::default(),
            deferred_bootstrap: Default::default(),
        })
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn backend(&self) -> SqliteBackend {
        self.backend
    }

    #[doc(hidden)]
    pub fn with_invocation_telemetry(
        mut self,
        telemetry: Option<crate::ActorInvocationTelemetry>,
    ) -> Self {
        self.invocation_telemetry = telemetry;
        self
    }

    /// Runs one SQLite operation inside a `hamlet.sqlite.*` span when
    /// the current invocation is traced.
    pub(super) async fn traced<T>(
        &self,
        operation: SqliteOperation,
        future: impl Future<Output = Result<T>>,
    ) -> Result<T> {
        let Some(mut span) = self
            .invocation_telemetry
            .as_ref()
            .and_then(|telemetry| telemetry.start_sqlite(operation))
        else {
            return future.await;
        };
        let result = future.instrument(span.span()).await;
        span.finish(result.as_ref().err());
        result
    }

    pub async fn open(&self) -> Result<()> {
        self.open_with(false).await
    }

    /// Opens the database of an actor whose start showed its storage
    /// was never written. A new database has no pages, so the local
    /// backend skips reading them; a database that does hold pages
    /// fails its first write on the head fence instead of being
    /// overwritten.
    pub async fn open_new(&self) -> Result<()> {
        self.open_with(true).await
    }

    async fn open_with(&self, _new: bool) -> Result<()> {
        match self.backend {
            SqliteBackend::LocalNative => {
                Err(SqliteRuntimeError::Unavailable.build())
            }
            SqliteBackend::RemoteEnvoy => {
                self.remote_config()?;
                Ok(())
            }
        }
    }

    async fn local_exec(&self, _sql: String) -> Result<QueryResult> {
        Err(SqliteRuntimeError::Unavailable.build())
    }

    async fn local_execute(
        &self,
        _sql: String,
        _params: Option<Vec<BindParam>>,
    ) -> Result<ExecuteResult> {
        Err(SqliteRuntimeError::Unavailable.build())
    }

    async fn local_execute_batch(
        &self,
        _statements: Vec<SqliteBatchStatement>,
    ) -> Result<()> {
        Err(SqliteRuntimeError::Unavailable.build())
    }

    pub async fn exec(
        &self,
        sql: impl Into<String>,
    ) -> Result<QueryResult> {
        let sql = sql.into();
        self.traced(SqliteOperation::Exec, self.exec_untraced(sql))
            .await
    }

    async fn exec_untraced(
        &self,
        sql: String,
    ) -> Result<QueryResult> {
        let sql_for_log = sql.clone();
        let guard = self.begin_regular_operation().await;
        let result = match guard {
            Ok(_guard) => self.exec_backend(sql).await,
            Err(error) => Err(error),
        };
        match result {
            Ok(result) => Ok(result),
            Err(error) => {
                let error = self.attach_actor(error);
                self.log_operation_error(
                    "exec",
                    &sql_for_log,
                    0,
                    &error,
                );
                Err(error)
            }
        }
    }

    pub async fn query(
        &self,
        sql: impl Into<String>,
        params: Option<Vec<BindParam>>,
    ) -> Result<QueryResult> {
        let sql = sql.into();
        self.traced(
            SqliteOperation::Query,
            self.query_untraced(sql, params),
        )
        .await
    }

    async fn query_untraced(
        &self,
        sql: String,
        params: Option<Vec<BindParam>>,
    ) -> Result<QueryResult> {
        let sql_for_log = sql.clone();
        let binding_count = bind_param_count(&params);
        let guard = self.begin_regular_operation().await;
        let result = match guard {
            Ok(_guard) => self.query_backend(sql, params).await,
            Err(error) => Err(error),
        };
        match result {
            Ok(result) => Ok(result),
            Err(error) => {
                let error = self.attach_actor(error);
                self.log_operation_error(
                    "query",
                    &sql_for_log,
                    binding_count,
                    &error,
                );
                Err(error)
            }
        }
    }

    pub async fn run(
        &self,
        sql: impl Into<String>,
        params: Option<Vec<BindParam>>,
    ) -> Result<ExecResult> {
        let sql = sql.into();
        self.traced(
            SqliteOperation::Run,
            self.run_untraced(sql, params),
        )
        .await
    }

    async fn run_untraced(
        &self,
        sql: String,
        params: Option<Vec<BindParam>>,
    ) -> Result<ExecResult> {
        let sql_for_log = sql.clone();
        let binding_count = bind_param_count(&params);
        let guard = self.begin_regular_operation().await;
        let result = match guard {
            Ok(_guard) => self.run_backend(sql, params).await,
            Err(error) => Err(error),
        };
        match result {
            Ok(result) => Ok(result),
            Err(error) => {
                let error = self.attach_actor(error);
                self.log_operation_error(
                    "run",
                    &sql_for_log,
                    binding_count,
                    &error,
                );
                Err(error)
            }
        }
    }

    pub async fn execute(
        &self,
        sql: impl Into<String>,
        params: Option<Vec<BindParam>>,
    ) -> Result<ExecuteResult> {
        let sql = sql.into();
        self.traced(
            SqliteOperation::Execute,
            self.execute_untraced(sql, params),
        )
        .await
    }

    async fn execute_untraced(
        &self,
        sql: String,
        params: Option<Vec<BindParam>>,
    ) -> Result<ExecuteResult> {
        let sql_for_log = sql.clone();
        let binding_count = bind_param_count(&params);
        let guard = self.begin_regular_operation().await;
        let result = match guard {
            Ok(_guard) => self.execute_backend(sql, params).await,
            Err(error) => Err(error),
        };
        match result {
            Ok(result) => Ok(result),
            Err(error) => {
                let error = self.attach_actor(error);
                self.log_operation_error(
                    "execute",
                    &sql_for_log,
                    binding_count,
                    &error,
                );
                Err(error)
            }
        }
    }

    pub async fn execute_batch(
        &self,
        statements: Vec<SqliteBatchStatement>,
    ) -> Result<Vec<ExecuteResult>> {
        self.traced(
            SqliteOperation::ExecuteBatch,
            self.execute_batch_untraced(statements),
        )
        .await
    }

    async fn execute_batch_untraced(
        &self,
        statements: Vec<SqliteBatchStatement>,
    ) -> Result<Vec<ExecuteResult>> {
        let statement_count = statements.len();
        let binding_count = statements
            .iter()
            .map(|statement| bind_param_count(&statement.params))
            .sum();
        let result = if self.backend == SqliteBackend::RemoteEnvoy {
            self.remote_execute_batch_with_bootstrap(statements).await
        } else {
            // The batch runs as its own task because its transaction
            // holds the coordinator gate. An actor task that stops
            // polling this future to await another statement would
            // otherwise wait on its own gate until the transaction
            // expires.
            let db = self.clone().with_invocation_telemetry(None);
            let batch = async move {
                let transaction = db.begin_transaction(None).await?;
                let mut results =
                    Vec::with_capacity(statements.len());
                for statement in statements {
                    match transaction
                        .execute(statement.sql, statement.params)
                        .await
                    {
                        Ok(result) => results.push(result),
                        Err(error) => {
                            return match transaction.rollback().await
                            {
                                Ok(()) => Err(error.context(
                                    "execute sqlite batch statement",
                                )),
                                Err(rollback_error) => Err(error
                                    .context(
                                        "execute sqlite batch \
                                         statement",
                                    )
                                    .context(
                                        rollback_error.context(
                                            "rollback sqlite batch \
                                             transaction",
                                        ),
                                    )),
                            };
                        }
                    }
                }
                transaction
                    .commit()
                    .await
                    .context("commit sqlite batch transaction")?;
                Ok(results)
            };
            run_detached_transaction_task(
                batch,
                "sqlite batch transaction task failed",
            )
            .await
        };

        match result {
            Ok(results) => Ok(results),
            Err(error) => {
                let error = self.attach_actor(error);
                self.log_operation_error_with_count(
                    "execute_batch",
                    "<batch>",
                    binding_count,
                    statement_count,
                    &error,
                );
                Err(error)
            }
        }
    }

    /// Moves this database's later work behind foreground work on the
    /// shared pool. Called when the actor starts shutting down, so
    /// its teardown never delays other actors' starts.
    pub(crate) fn mark_background(&self) {}

    /// Asks the shared pool to return freed native memory to the OS;
    /// see `SqlitePool::request_allocator_trim`.
    pub(crate) fn request_allocator_trim(&self) {}

    /// Makes the commits held since `cork_commits` durable as one
    /// depot commit. An open explicit transaction delays it until the
    /// transaction ends, so the commit includes it.
    pub(crate) async fn uncork_commits(&self) -> Result<()> {
        Ok(())
    }

    /// Interrupts the statement a failed start's hooks left running
    /// on the local worker, without closing the database, so the
    /// commands the teardown queues before its close, such as
    /// [`Self::discard_corked_commits`], do not wait for that
    /// statement. Marking the close instead would fail the queued
    /// discard, and closing the connection then sends the held
    /// commits to depot. The interrupt is sticky, so the database
    /// must close afterwards. A remote database has no local worker,
    /// so this does nothing there.
    pub(crate) fn interrupt(&self) {}

    /// Drops the commits held since `cork_commits` without sending
    /// them, for a start that failed before they were meant to become
    /// durable. The database is unusable afterwards and must
    /// close: every later SQL command fails as `sqlite.closed`, also
    /// one already queued behind the discard, and the close drops the
    /// held commits even when the discard itself failed. It
    /// is never refused for a full command queue. A database that
    /// never corked has nothing held, so this does nothing there.
    pub(crate) async fn discard_corked_commits(&self) -> Result<()> {
        Ok(())
    }

    pub async fn close(&self) -> Result<()> {
        let db = self.clone();
        run_detached_transaction_task(
            async move {
                let _gate =
                    db.shutdown_transaction_coordinator().await;
                db.close_backend(NativeClose::Bounded).await
            },
            "sqlite close task failed",
        )
        .await
    }

    /// Closes SQLite without returning before the native worker
    /// terminates. Any returned error is reported only after the
    /// worker has relinquished SQLite.
    pub async fn close_and_wait(&self) -> Result<()> {
        // Keep this strict path in the caller task. Wrapping it in a
        // detached task would make a task-level join failure
        // ambiguous: the caller could no longer prove that the worker
        // had relinquished SQLite before replacing the actor.
        let _gate = self.shutdown_transaction_coordinator().await;
        self.close_backend(NativeClose::Strict).await
    }

    /// Closes SQLite like [`Self::close_and_wait`] for a teardown
    /// that cannot wait on user code, such as a failed start whose
    /// hook left a statement running. Waiting for the coordinator
    /// to drain stops after `limit`, since a running statement holds
    /// the coordinator until its reply comes, which may be never:
    /// inside a transaction it holds the transaction's
    /// statement lock, and outside one it holds the coordinator's
    /// gate.
    ///
    /// The worker close stays strict: this still returns only once
    /// the native worker relinquished SQLite, which a new generation
    /// needs as proof. For `LocalNative` a stuck statement is a
    /// command running on the pool thread. The close interrupts it,
    /// which is safe because the connection belongs to this
    /// generation and nothing wants the statement's outcome. The
    /// command then stops at its next progress callback, or once the
    /// VFS request it is blocked on returns. The VFS transport's
    /// request deadline (the envoy client's `KV_EXPIRE_MS`, 30 s)
    /// bounds that one request, not the statement, so the strict
    /// close waits for roughly one request deadline rather than the
    /// whole statement. A transport without a deadline would hold
    /// this close for as long as its request. A remote database
    /// has no local worker to wait for, so `limit` bounds its whole
    /// close.
    pub(crate) async fn close_and_wait_within(
        &self,
        limit: Duration,
    ) -> Result<()> {
        let gate =
            self.shutdown_transaction_coordinator_within(limit).await;
        if let Err(timed_out) = &gate {
            tracing::warn!(
                actor_id =
                    self.actor_id.as_deref().unwrap_or("unknown"),
                limit_ms = limit.as_millis() as u64,
                transaction_active = timed_out.transaction_active,
                "a statement still holds the sqlite coordinator \
                 while sqlite closes; closing without waiting for \
                 it, and any transaction it holds never commits"
            );
        }
        let result =
            self.close_backend(NativeClose::StrictInterrupting).await;
        drop(gate);
        result
    }

    async fn close_backend(&self, _close: NativeClose) -> Result<()> {
        match self.backend {
            SqliteBackend::LocalNative
            | SqliteBackend::RemoteEnvoy => Ok(()),
        }
    }

    pub(crate) async fn cleanup_for_shutdown(
        &self,
        reusable: bool,
    ) -> Result<()> {
        // A remote database has no actor-local resources to release.
        // Keep its coordinator usable across a sleep/wake cycle on
        // the same ActorTask.
        if reusable && self.backend == SqliteBackend::RemoteEnvoy {
            return Ok(());
        }
        self.close().await
    }

    #[cfg(test)]
    pub(crate) fn fresh_remote_for_test(&self) -> Self {
        Self::new_with_remote_sqlite(
            self.handle.clone().expect(
                "remote sqlite test database should have an envoy \
                 handle",
            ),
            self.actor_id.clone().expect(
                "remote sqlite test database should have an actor id",
            ),
            self.actor_key.clone(),
            self.generation,
            self.enabled,
            true,
        )
        .expect("remote sqlite test database should be configured")
    }

    pub fn take_last_kv_error(&self) -> Option<String> {
        if self.backend != SqliteBackend::LocalNative {
            return None;
        }

        None
    }

    /// Subscribes to whether the connection has an open explicit
    /// transaction, which includes a manual `BEGIN` sent through
    /// ordinary statements. Returns `None` when the backend cannot
    /// report it: the remote backend, or a local database that is not
    /// open.
    pub(crate) fn subscribe_explicit_transaction(
        &self,
    ) -> Option<watch::Receiver<bool>> {
        if self.backend != SqliteBackend::LocalNative {
            return None;
        }

        None
    }

    pub fn metrics(&self) -> Option<SqliteVfsMetricsSnapshot> {
        None
    }

    pub fn runtime_config(&self) -> Result<SqliteRuntimeConfig> {
        Ok(SqliteRuntimeConfig {
            handle: self.handle()?,
            actor_id: self
                .actor_id
                .clone()
                .ok_or_else(|| sqlite_not_configured("actor id"))?,
            generation: self.generation,
        })
    }

    fn log_operation_error(
        &self,
        operation: &'static str,
        sql: &str,
        binding_count: usize,
        error: &anyhow::Error,
    ) {
        self.log_operation_error_with_count(
            operation,
            sql,
            binding_count,
            1,
            error,
        );
    }

    fn log_operation_error_with_count(
        &self,
        operation: &'static str,
        sql: &str,
        binding_count: usize,
        statement_count: usize,
        error: &anyhow::Error,
    ) {
        let structured = ActorError::extract(error);
        // Engine rollouts can disconnect after a mutation was sent
        // but before its response arrives. The transport already
        // reports the disconnect, so keep this operation-level event
        // compact.
        if structured.group() == "sqlite"
            && structured.code() == "remote_indeterminate_result"
        {
            tracing::debug!(
                actor_id = self.actor_id.as_deref().unwrap_or("<unknown>"),
                generation = ?self.generation,
                backend = ?self.backend,
                operation,
                "sqlite operation result indeterminate after envoy disconnect"
            );
        } else {
            let error_chain = error
                .chain()
                .map(ToString::to_string)
                .collect::<Vec<_>>();
            tracing::error!(
                actor_id = self.actor_id.as_deref().unwrap_or("<unknown>"),
                generation = ?self.generation,
                backend = ?self.backend,
                operation,
                sql,
                binding_count,
                statement_count,
                group = structured.group(),
                code = structured.code(),
                error_message = %structured.message(),
                metadata = ?structured.metadata(),
                error_chain = ?error_chain,
                "sqlite operation failed"
            );
        }
    }

    fn actor_specifier(&self) -> Option<ActorSpecifier> {
        let mut specifier = ActorSpecifier::new(
            self.actor_id.as_ref()?.clone(),
            self.generation?,
        );
        if let Some(key) = self.actor_key.as_ref() {
            specifier = specifier.with_key(key.clone());
        }
        Some(specifier)
    }

    fn attach_actor(&self, error: anyhow::Error) -> anyhow::Error {
        match self.actor_specifier() {
            Some(actor) => error.context(actor),
            None => error,
        }
    }

    fn remote_config(&self) -> Result<RemoteSqliteConfig> {
        let config = self.runtime_config()?;
        let generation = config
            .generation
            .ok_or_else(|| sqlite_not_configured("generation"))?;
        Ok(RemoteSqliteConfig {
            namespace_id: config.handle.namespace().to_owned(),
            handle: config.handle,
            actor_id: config.actor_id,
            generation,
        })
    }

    async fn remote_exec(&self, sql: String) -> Result<QueryResult> {
        let config = self.remote_config()?;
        config
            .handle
            .sqlite_exec(config.actor_id, sql)
            .await
            .map_err(remote_request_error)
    }

    async fn remote_exec_with_session(
        &self,
        sql: String,
        _expected_session: Option<u64>,
    ) -> Result<(QueryResult, u64)> {
        let config = self.remote_config()?;
        let session =
            config.handle.connection_session().unwrap_or_default();
        let result =
            config.handle.sqlite_exec(config.actor_id, sql).await?;
        Ok((result, session))
    }

    async fn remote_execute(
        &self,
        sql: String,
        params: Option<Vec<BindParam>>,
    ) -> Result<ExecuteResult> {
        let config = self.remote_config()?;
        config
            .handle
            .sqlite_execute(config.actor_id, sql, params)
            .await
            .map_err(remote_request_error)
    }

    async fn remote_execute_batch(
        &self,
        statements: Vec<SqliteBatchStatement>,
    ) -> Result<Vec<ExecuteResult>> {
        let config = self.remote_config()?;
        let results = config
            .handle
            .sqlite_execute_batch(config.actor_id.clone(), statements)
            .await
            .map_err(remote_request_error)?;
        config.handle.transaction_committed(&config.actor_id);
        Ok(results)
    }

    async fn remote_execute_with_session(
        &self,
        sql: String,
        params: Option<Vec<BindParam>>,
        _expected_session: Option<u64>,
    ) -> Result<(ExecuteResult, u64)> {
        let config = self.remote_config()?;
        let session =
            config.handle.connection_session().unwrap_or_default();
        let result = config
            .handle
            .sqlite_execute(config.actor_id, sql, params)
            .await?;
        Ok((result, session))
    }

    pub(crate) async fn query_rows_cbor(
        &self,
        sql: &str,
        params: Option<&[u8]>,
    ) -> Result<Vec<u8>> {
        let bind_params = bind_params_from_cbor(sql, params)?;
        let result = self.query(sql.to_owned(), bind_params).await?;
        encode_json_as_cbor(&query_result_to_json_rows(&result))
    }

    pub(crate) async fn exec_rows_cbor(
        &self,
        sql: &str,
    ) -> Result<Vec<u8>> {
        let result = self.exec(sql.to_owned()).await?;
        encode_json_as_cbor(&query_result_to_json_rows(&result))
    }

    pub(crate) async fn run_cbor(
        &self,
        sql: &str,
        params: Option<&[u8]>,
    ) -> Result<ExecResult> {
        let bind_params = bind_params_from_cbor(sql, params)?;
        self.run(sql.to_owned(), bind_params).await
    }

    pub(crate) async fn execute_rows_cbor(
        &self,
        sql: &str,
        params: Option<&[u8]>,
    ) -> Result<Vec<u8>> {
        let bind_params = bind_params_from_cbor(sql, params)?;
        let result =
            self.execute(sql.to_owned(), bind_params).await?;
        encode_json_as_cbor(&query_result_to_json_rows(
            &QueryResult {
                columns: result
                    .columns
                    .into_iter()
                    .map(|column| column.to_string())
                    .collect(),
                rows: result.rows,
            },
        ))
    }

    fn handle(&self) -> Result<EnvoyHandle> {
        self.handle
            .clone()
            .ok_or_else(|| sqlite_not_configured("handle"))
    }
}

fn report_sqlite_worker_fatal(
    reported: &AtomicBool,
    config: SqliteRuntimeConfig,
    message: String,
) {
    if reported.swap(true, Ordering::AcqRel) {
        return;
    }
    // A dead worker means SQLite's sole native connection is no
    // longer a valid actor subsystem. Core reports that through envoy
    // lifecycle instead of letting the actor continue to serve
    // requests with a broken database. This is a crash, not a
    // deliberate destroy, so it goes out as a sleep intent: the next
    // generation opens a fresh worker over the same durable state.
    config.handle.sleep_actor(
        config.actor_id,
        config
            .generation
            .and_then(|generation| generation.try_into().ok()),
        Some(message),
    );
}

struct RemoteSqliteConfig {
    handle: EnvoyHandle,
    #[expect(
        dead_code,
        reason = "ported; nothing in Hamlet calls it yet"
    )]
    namespace_id: String,
    actor_id: String,
    #[expect(
        dead_code,
        reason = "ported; nothing in Hamlet calls it yet"
    )]
    generation: u64,
}

fn select_sqlite_backend(
    remote_sqlite: bool,
) -> Result<SqliteBackend> {
    if remote_sqlite {
        return Ok(SqliteBackend::RemoteEnvoy);
    }

    Err(SqliteRuntimeError::Unavailable.build())
}

fn bind_param_count(params: &Option<Vec<BindParam>>) -> usize {
    params.as_ref().map_or(0, Vec::len)
}

fn remote_request_error(error: anyhow::Error) -> anyhow::Error {
    if let Some(indeterminate) =
        error.downcast_ref::<RemoteSqliteIndeterminateResultError>()
    {
        return SqliteRuntimeError::RemoteIndeterminateResult {
            operation: indeterminate.operation.to_owned(),
        }
        .build();
    }

    error
}
impl std::fmt::Debug for SqliteDb {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        f.debug_struct("SqliteDb")
            .field("configured", &self.handle.is_some())
            .field("actor_id", &self.actor_id)
            .finish()
    }
}

fn bind_params_from_cbor(
    sql: &str,
    params: Option<&[u8]>,
) -> Result<Option<Vec<BindParam>>> {
    let Some(params) = params else {
        return Ok(None);
    };
    if params.is_empty() {
        return Ok(None);
    }

    let value =
        ciborium::from_reader::<JsonValue, _>(Cursor::new(params))
            .context("decode sqlite bind params as cbor json")?;
    match value {
        JsonValue::Array(values) => values
            .iter()
            .map(json_to_bind_param)
            .collect::<Result<Vec<_>>>()
            .map(Some),
        JsonValue::Object(properties) => {
            let ordered_names = extract_named_sqlite_parameters(sql);
            if ordered_names.is_empty() {
                return properties
                    .values()
                    .map(json_to_bind_param)
                    .collect::<Result<Vec<_>>>()
                    .map(Some);
            }

            ordered_names
                .iter()
                .map(|name| {
                    get_named_sqlite_binding(&properties, name)
                        .ok_or_else(|| {
                            SqliteRuntimeError::InvalidBindParameter {
                                name: name.clone(),
                                reason: "missing parameter"
                                    .to_owned(),
                            }
                            .build()
                        })
                        .and_then(json_to_bind_param)
                })
                .collect::<Result<Vec<_>>>()
                .map(Some)
        }
        JsonValue::Null => Ok(None),
        other => Err(SqliteRuntimeError::InvalidBindParameter {
            name: "params".to_owned(),
            reason: format!(
                "expected array or object, got {}",
                json_type_name(&other)
            ),
        }
        .build()),
    }
}

fn json_to_bind_param(value: &JsonValue) -> Result<BindParam> {
    match value {
        JsonValue::Null => Ok(BindParam::Null),
        JsonValue::Bool(value) => {
            Ok(BindParam::Integer(i64::from(*value)))
        }
        JsonValue::Number(value) => {
            if let Some(value) = value.as_i64() {
                return Ok(BindParam::Integer(value));
            }
            if let Some(value) = value.as_u64() {
                let value = i64::try_from(value).context(
                    "sqlite integer bind parameter exceeds i64 range",
                )?;
                return Ok(BindParam::Integer(value));
            }
            value.as_f64().map(BindParam::Float).ok_or_else(|| {
                SqliteRuntimeError::InvalidBindParameter {
                    name: "number".to_owned(),
                    reason: "unsupported numeric value".to_owned(),
                }
                .build()
            })
        }
        JsonValue::String(value) => {
            Ok(BindParam::Text(value.clone()))
        }
        other => Err(SqliteRuntimeError::InvalidBindParameter {
            name: "value".to_owned(),
            reason: format!(
                "unsupported type {}",
                json_type_name(other)
            ),
        }
        .build()),
    }
}

fn sqlite_not_configured(component: &str) -> anyhow::Error {
    SqliteRuntimeError::NotConfigured {
        component: component.to_owned(),
    }
    .build()
}

fn extract_named_sqlite_parameters(sql: &str) -> Vec<String> {
    let mut ordered_names = Vec::new();
    let mut seen = HashSet::new();
    let bytes = sql.as_bytes();
    let mut idx = 0;

    while idx < bytes.len() {
        let byte = bytes[idx];
        if !matches!(byte, b':' | b'@' | b'$') {
            idx += 1;
            continue;
        }

        let start = idx;
        idx += 1;
        if idx >= bytes.len() || !is_sqlite_param_start(bytes[idx]) {
            continue;
        }
        idx += 1;
        while idx < bytes.len()
            && is_sqlite_param_continue(bytes[idx])
        {
            idx += 1;
        }

        let name = &sql[start..idx];
        if seen.insert(name.to_owned()) {
            ordered_names.push(name.to_owned());
        }
    }

    ordered_names
}

fn is_sqlite_param_start(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphabetic()
}

fn is_sqlite_param_continue(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphanumeric()
}

fn get_named_sqlite_binding<'a>(
    bindings: &'a JsonMap<String, JsonValue>,
    name: &str,
) -> Option<&'a JsonValue> {
    if let Some(value) = bindings.get(name) {
        return Some(value);
    }

    let bare_name = name.get(1..)?;
    if let Some(value) = bindings.get(bare_name) {
        return Some(value);
    }

    for prefix in [":", "@", "$"] {
        let candidate = format!("{prefix}{bare_name}");
        if let Some(value) = bindings.get(&candidate) {
            return Some(value);
        }
    }

    None
}

fn query_result_to_json_rows(result: &QueryResult) -> JsonValue {
    JsonValue::Array(
        result
            .rows
            .iter()
            .map(|row| {
                let mut object = JsonMap::new();
                for (index, column) in
                    result.columns.iter().enumerate()
                {
                    let value = row
                        .get(index)
                        .map(column_value_to_json)
                        .unwrap_or(JsonValue::Null);
                    object.insert(column.clone(), value);
                }
                JsonValue::Object(object)
            })
            .collect(),
    )
}

fn column_value_to_json(value: &ColumnValue) -> JsonValue {
    match value {
        ColumnValue::Null => JsonValue::Null,
        ColumnValue::Integer(value) => JsonValue::from(*value),
        ColumnValue::Float(value) => JsonValue::from(*value),
        ColumnValue::Text(value) => JsonValue::String(value.clone()),
        ColumnValue::Blob(value) => JsonValue::Array(
            value.iter().map(|byte| JsonValue::from(*byte)).collect(),
        ),
    }
}

fn encode_json_as_cbor(value: &impl Serialize) -> Result<Vec<u8>> {
    let mut encoded = Vec::new();
    ciborium::into_writer(value, &mut encoded)
        .context("encode sqlite rows as cbor")?;
    Ok(encoded)
}

fn json_type_name(value: &JsonValue) -> &'static str {
    match value {
        JsonValue::Null => "null",
        JsonValue::Bool(_) => "boolean",
        JsonValue::Number(_) => "number",
        JsonValue::String(_) => "string",
        JsonValue::Array(_) => "array",
        JsonValue::Object(_) => "object",
    }
}

#[cfg(test)]
#[path = "../../tests/sqlite.rs"]
mod tests;
