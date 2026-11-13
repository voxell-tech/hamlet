// This file was changed for Hamlet. See NOTICE in this crate.
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

use tokio::sync::{mpsc, oneshot, watch};
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context as LayerContext, Layer};
use tracing_subscriber::prelude::*;
use tracing_subscriber::registry::Registry;

use super::*;
use crate::host::ActorHost;
use crate::runtime::RuntimeBoxFuture;

#[derive(Clone, Debug, Default)]
struct SqliteOperationLog {
    level: Option<tracing::Level>,
    message: Option<String>,
    actor_id: Option<String>,
    generation: Option<String>,
    backend: Option<String>,
    operation: Option<String>,
    sql: Option<String>,
    binding_count: Option<u64>,
    group: Option<String>,
    code: Option<String>,
    error_message: Option<String>,
}

#[derive(Clone)]
struct SqliteOperationLogLayer {
    records: Arc<StdMutex<Vec<SqliteOperationLog>>>,
}

#[derive(Default)]
struct SqliteOperationLogVisitor {
    record: SqliteOperationLog,
}

impl Visit for SqliteOperationLogVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        match field.name() {
            "message" => self.record.message = Some(value.to_owned()),
            "actor_id" => {
                self.record.actor_id = Some(value.to_owned());
            }
            "backend" => self.record.backend = Some(value.to_owned()),
            "operation" => {
                self.record.operation = Some(value.to_owned());
            }
            "sql" => self.record.sql = Some(value.to_owned()),
            "group" => self.record.group = Some(value.to_owned()),
            "code" => self.record.code = Some(value.to_owned()),
            "error_message" => {
                self.record.error_message = Some(value.to_owned());
            }
            _ => {}
        }
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        if field.name() == "binding_count" {
            self.record.binding_count = Some(value);
        }
    }

    fn record_debug(
        &mut self,
        field: &Field,
        value: &dyn std::fmt::Debug,
    ) {
        match field.name() {
            "message" => {
                self.record.message = Some(
                    format!("{value:?}").trim_matches('"').to_owned(),
                );
            }
            "generation" => {
                self.record.generation = Some(format!("{value:?}"));
            }
            "backend" => {
                self.record.backend = Some(format!("{value:?}"));
            }
            "error_message" => {
                self.record.error_message = Some(
                    format!("{value:?}").trim_matches('"').to_owned(),
                );
            }
            _ => {}
        }
    }
}

impl<S> Layer<S> for SqliteOperationLogLayer
where
    S: Subscriber,
{
    fn on_event(&self, event: &Event<'_>, _ctx: LayerContext<'_, S>) {
        let mut visitor = SqliteOperationLogVisitor::default();
        event.record(&mut visitor);
        visitor.record.level = Some(*event.metadata().level());
        self.records
            .lock()
            .expect("sqlite operation log lock poisoned")
            .push(visitor.record);
    }
}

/// What the database asked of the test host: a remote SQLite request
/// with the sender for its response, or an actor intent such as the
/// sleep a fatal SQLite error reports.
enum ToEnvoyMessage {
    RemoteSqliteRequest(RemoteSqliteRequest),
    ActorIntent(String),
}

enum RemoteSqliteRequest {
    Exec {
        request: SqliteExecuteRequest,
        response_tx: oneshot::Sender<Result<QueryResult>>,
    },
    Execute {
        request: SqliteExecuteRequest,
        response_tx: oneshot::Sender<Result<ExecuteResult>>,
    },
    ExecuteBatch {
        request: SqliteExecuteBatchRequest,
        response_tx: oneshot::Sender<Result<Vec<ExecuteResult>>>,
    },
}

struct SqliteExecuteRequest {
    sql: String,
}

struct SqliteExecuteBatchRequest {
    statements: Vec<SqliteBatchStatement>,
}

/// A host whose SQLite requests the test answers by hand, and whose
/// connection session the test moves to simulate a disconnect.
struct TestEnvoy {
    envoy_tx: mpsc::UnboundedSender<ToEnvoyMessage>,
    connection_session: AtomicU64,
    connection_session_tx: watch::Sender<u64>,
}

impl TestEnvoy {
    fn request<T: Send + 'static>(
        &self,
        request: impl FnOnce(
            oneshot::Sender<Result<T>>,
        ) -> RemoteSqliteRequest
        + Send
        + 'static,
    ) -> RuntimeBoxFuture<Result<T>> {
        let envoy_tx = self.envoy_tx.clone();
        Box::pin(async move {
            let (response_tx, response_rx) = oneshot::channel();
            envoy_tx
                .send(ToEnvoyMessage::RemoteSqliteRequest(request(
                    response_tx,
                )))
                .map_err(|_| {
                    anyhow::anyhow!("envoy channel closed")
                })?;
            response_rx.await.map_err(|_| {
                anyhow::anyhow!("remote sqlite response dropped")
            })?
        })
    }

    fn intent(&self, intent: String) {
        let _ =
            self.envoy_tx.send(ToEnvoyMessage::ActorIntent(intent));
    }
}

impl ActorHost for TestEnvoy {
    fn endpoint(&self) -> &str {
        "http://127.0.0.1:1"
    }

    fn namespace(&self) -> &str {
        "test"
    }

    fn pool_name(&self) -> &str {
        "test"
    }

    fn connection_session(&self) -> Option<u64> {
        let session = self.connection_session.load(Ordering::Acquire);
        (session != 0).then_some(session)
    }

    fn subscribe_connection_session(&self) -> watch::Receiver<u64> {
        self.connection_session_tx.subscribe()
    }

    fn sleep_actor(
        &self,
        actor_id: String,
        generation: Option<u32>,
        error: Option<String>,
    ) {
        self.intent(format!(
            "sleep {actor_id} {generation:?}: {error:?}"
        ));
    }

    fn stop_actor(&self, actor_id: String, generation: Option<u32>) {
        self.intent(format!("stop {actor_id} {generation:?}"));
    }

    fn set_alarm_with_ack(
        &self,
        _actor_id: String,
        _alarm_ts: Option<i64>,
        _generation: Option<u32>,
        _ack_tx: Option<oneshot::Sender<()>>,
    ) {
        unreachable!("sqlite tests set no alarm")
    }

    fn sqlite_exec(
        &self,
        _actor_id: String,
        sql: String,
    ) -> RuntimeBoxFuture<Result<QueryResult>> {
        self.request(|response_tx| RemoteSqliteRequest::Exec {
            request: SqliteExecuteRequest { sql },
            response_tx,
        })
    }

    fn sqlite_execute(
        &self,
        _actor_id: String,
        sql: String,
        _params: Option<Vec<BindParam>>,
    ) -> RuntimeBoxFuture<Result<ExecuteResult>> {
        self.request(|response_tx| RemoteSqliteRequest::Execute {
            request: SqliteExecuteRequest { sql },
            response_tx,
        })
    }

    fn sqlite_execute_batch(
        &self,
        _actor_id: String,
        statements: Vec<SqliteBatchStatement>,
    ) -> RuntimeBoxFuture<Result<Vec<ExecuteResult>>> {
        self.request(|response_tx| {
            RemoteSqliteRequest::ExecuteBatch {
                request: SqliteExecuteBatchRequest { statements },
                response_tx,
            }
        })
    }
}

fn test_envoy_handle_with_shared() -> (
    EnvoyHandle,
    mpsc::UnboundedReceiver<ToEnvoyMessage>,
    Arc<TestEnvoy>,
) {
    let (envoy_tx, envoy_rx) = mpsc::unbounded_channel();
    let shared = Arc::new(TestEnvoy {
        envoy_tx,
        connection_session: AtomicU64::new(1),
        connection_session_tx: watch::channel(1).0,
    });

    (shared.clone(), envoy_rx, shared)
}

fn test_envoy_handle()
-> (EnvoyHandle, mpsc::UnboundedReceiver<ToEnvoyMessage>) {
    let (handle, envoy_rx, _) = test_envoy_handle_with_shared();
    (handle, envoy_rx)
}

async fn respond_to_execute(
    envoy_rx: &mut mpsc::UnboundedReceiver<ToEnvoyMessage>,
    expected_sql: &str,
) {
    let response_tx = receive_execute(envoy_rx, expected_sql).await;
    send_execute_ok(response_tx);
}

async fn wait_for_coordinator_state(
    db: &SqliteDb,
    expectation: &'static str,
    predicate: impl Fn(&TransactionCoordinatorState) -> bool,
) {
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let ready = {
                let state =
                    db.transaction_coordinator.state.lock().await;
                predicate(&state)
            };
            if ready {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect(expectation);
}

async fn receive_message(
    envoy_rx: &mut mpsc::UnboundedReceiver<ToEnvoyMessage>,
) -> RemoteSqliteRequest {
    let message =
        tokio::time::timeout(Duration::from_secs(2), envoy_rx.recv())
            .await
            .expect("timed out waiting for remote sqlite request")
            .expect("envoy request channel closed");
    match message {
        ToEnvoyMessage::RemoteSqliteRequest(request) => request,
        ToEnvoyMessage::ActorIntent(intent) => {
            panic!("expected remote sqlite request, got {intent}")
        }
    }
}

async fn receive_execute(
    envoy_rx: &mut mpsc::UnboundedReceiver<ToEnvoyMessage>,
    expected_sql: &str,
) -> oneshot::Sender<Result<ExecuteResult>> {
    let RemoteSqliteRequest::Execute {
        request,
        response_tx,
    } = receive_message(envoy_rx).await
    else {
        panic!("expected remote sqlite execute request");
    };
    assert_eq!(request.sql, expected_sql);
    response_tx
}

fn send_execute_ok(
    response_tx: oneshot::Sender<Result<ExecuteResult>>,
) {
    response_tx
        .send(Ok(ExecuteResult {
            columns: Vec::new(),
            rows: Vec::new(),
            changes: 0,
            last_insert_row_id: None,
        }))
        .expect("remote sqlite requester dropped response");
}

async fn receive_execute_batch(
    envoy_rx: &mut mpsc::UnboundedReceiver<ToEnvoyMessage>,
) -> (
    SqliteExecuteBatchRequest,
    oneshot::Sender<Result<Vec<ExecuteResult>>>,
) {
    let RemoteSqliteRequest::ExecuteBatch {
        request,
        response_tx,
    } = receive_message(envoy_rx).await
    else {
        panic!("expected remote sqlite execute batch request");
    };
    (request, response_tx)
}

fn send_execute_batch_ok(
    response_tx: oneshot::Sender<Result<Vec<ExecuteResult>>>,
    result_count: usize,
) {
    let result = ExecuteResult {
        columns: Vec::new(),
        rows: Vec::new(),
        changes: 0,
        last_insert_row_id: None,
    };
    response_tx
        .send(Ok(vec![result; result_count]))
        .expect("remote sqlite requester dropped response");
}

#[test]
fn remote_backend_selection_is_independent_of_user_database_flag() {
    assert_eq!(
        select_sqlite_backend(true)
            .expect("remote sqlite should always be available"),
        SqliteBackend::RemoteEnvoy
    );
    assert_eq!(
        select_sqlite_backend(true).expect(
            "remote sqlite should ignore public database opt-in"
        ),
        SqliteBackend::RemoteEnvoy
    );

    let error = select_sqlite_backend(false).expect_err(
        "construction without a sqlite backend must fail",
    );
    let error = ActorError::extract(&error);
    assert_eq!(error.group(), "sqlite");
    assert_eq!(error.code(), "unavailable");
}

#[tokio::test]
async fn transaction_arguments_are_structured_errors() {
    let (handle, _) = test_envoy_handle();
    let db = SqliteDb::new_with_remote_sqlite(
        handle,
        "actor-a",
        None,
        Some(7),
        true,
        true,
    )
    .expect("test remote sqlite should be configured");
    for error in [
        db.begin_transaction_with_key("", None)
            .await
            .err()
            .expect("empty key should fail"),
        db.begin_transaction_with_key("valid", Some(Duration::ZERO))
            .await
            .err()
            .expect("zero timeout should fail"),
        db.begin_named_transaction(Some(""), None)
            .await
            .err()
            .expect("empty name should fail"),
        db.begin_named_transaction(Some(&"x".repeat(129)), None)
            .await
            .err()
            .expect("oversized name should fail"),
    ] {
        let structured = ActorError::extract(&error);
        assert_eq!(structured.group(), "sqlite");
        assert_eq!(structured.code(), "transaction_invalid_argument");
    }
}

#[test]
fn remote_lost_response_errors_become_indeterminate_result() {
    let err = anyhow::anyhow!(RemoteSqliteIndeterminateResultError {
        operation: "execute",
    });

    let mapped = remote_request_error(err);
    let structured = ActorError::extract(&mapped);
    assert_eq!(structured.group(), "sqlite");
    assert_eq!(structured.code(), "remote_indeterminate_result");
}

// A lost response to a write outside a transaction is the same
// structured error a transaction's statement gets.
#[tokio::test]
async fn lost_response_outside_a_transaction_is_indeterminate() {
    let (handle, mut envoy_rx) = test_envoy_handle();
    let db = SqliteDb::new_with_remote_sqlite(
        handle,
        "actor-a",
        None,
        Some(7),
        true,
        true,
    )
    .expect("test remote sqlite should be configured");
    let lost = || {
        Err(anyhow::anyhow!(RemoteSqliteIndeterminateResultError {
            operation: "execute",
        }))
    };

    let execute = tokio::spawn({
        let db = db.clone();
        async move { db.execute("UPDATE t SET v = 1", None).await }
    });
    receive_execute(&mut envoy_rx, "UPDATE t SET v = 1")
        .await
        .send(lost())
        .expect("execute requester dropped response");
    let error = execute.await.unwrap().unwrap_err();
    assert_eq!(
        ActorError::extract(&error).code(),
        "remote_indeterminate_result"
    );

    let exec = tokio::spawn({
        let db = db.clone();
        async move { db.exec("UPDATE t SET v = 3").await }
    });
    let RemoteSqliteRequest::Exec {
        request,
        response_tx,
    } = receive_message(&mut envoy_rx).await
    else {
        panic!("expected remote sqlite exec request");
    };
    assert_eq!(request.sql, "UPDATE t SET v = 3");
    response_tx
        .send(Err(anyhow::anyhow!(
            RemoteSqliteIndeterminateResultError {
                operation: "exec"
            }
        )))
        .expect("exec requester dropped response");
    let error = exec.await.unwrap().unwrap_err();
    assert_eq!(
        ActorError::extract(&error).code(),
        "remote_indeterminate_result"
    );

    let batch = tokio::spawn({
        let db = db.clone();
        async move {
            db.execute_batch(vec![SqliteBatchStatement {
                sql: "UPDATE t SET v = 2".to_owned(),
                params: None,
            }])
            .await
        }
    });
    let (_, response_tx) = receive_execute_batch(&mut envoy_rx).await;
    response_tx
        .send(Err(anyhow::anyhow!(
            RemoteSqliteIndeterminateResultError {
                operation: "execute_batch",
            }
        )))
        .expect("batch requester dropped response");
    let error = batch.await.unwrap().unwrap_err();
    assert_eq!(
        ActorError::extract(&error).code(),
        "remote_indeterminate_result"
    );
}

#[tokio::test]
async fn remote_execute_logs_operation_context_at_source() {
    let (handle, envoy_rx) = test_envoy_handle();
    drop(envoy_rx);
    let db = SqliteDb::new_with_remote_sqlite(
        handle,
        "actor-sqlite-log",
        Some("user/1".to_owned()),
        Some(7),
        true,
        true,
    )
    .expect("test remote sqlite should be configured");
    let records = Arc::new(StdMutex::new(Vec::new()));
    let subscriber =
        Registry::default().with(SqliteOperationLogLayer {
            records: records.clone(),
        });
    let _guard = tracing::subscriber::set_default(subscriber);
    // With one registered subscriber, tracing caches a callsite's
    // interest from the first thread to reach it, which may be
    // another test's thread with none. A second one makes each event
    // ask its thread's subscriber.
    let _other = tracing::Dispatch::new(
        tracing::subscriber::NoSubscriber::default(),
    );

    let result = db
        .execute(
            "SELECT ?",
            Some(vec![
                BindParam::Integer(1),
                BindParam::Text("two".to_owned()),
            ]),
        )
        .await;

    assert!(result.is_err());
    let actor_specifier =
        ActorError::extract(&result.err().unwrap()).actor().cloned();
    assert_eq!(
        actor_specifier,
        Some(
            ActorSpecifier::new("actor-sqlite-log", 7)
                .with_key("user/1")
        )
    );
    let logs = records
        .lock()
        .expect("sqlite operation log lock poisoned")
        .clone();
    assert!(
        logs.iter().any(|log| {
            log.level == Some(tracing::Level::ERROR)
                && log.message.as_deref()
                    == Some("sqlite operation failed")
                && log.actor_id.as_deref() == Some("actor-sqlite-log")
                && log.generation.as_deref() == Some("Some(7)")
                && log.backend.as_deref() == Some("RemoteEnvoy")
                && log.operation.as_deref() == Some("execute")
                && log.sql.as_deref() == Some("SELECT ?")
                && log.binding_count == Some(2)
                && log.group.as_deref() == Some("core")
                && log.code.as_deref() == Some("internal_error")
                && log.error_message.as_deref()
                    == Some("An internal error occurred")
        }),
        "expected source sqlite operation log with actor id and \
         generation; logs={logs:?}"
    );
}

#[tokio::test]
async fn remote_execute_batch_uses_one_coordinated_transaction() {
    let (handle, mut envoy_rx) = test_envoy_handle();
    let db = SqliteDb::new_with_remote_sqlite(
        handle,
        "actor-a",
        None,
        Some(7),
        true,
        true,
    )
    .expect("test remote sqlite should be configured");

    let batch = tokio::spawn({
        let db = db.clone();
        async move {
            db.execute_batch(vec![
                SqliteBatchStatement {
                    sql: "insert-one".to_owned(),
                    params: None,
                },
                SqliteBatchStatement {
                    sql: "insert-two".to_owned(),
                    params: Some(vec![BindParam::Integer(2)]),
                },
            ])
            .await
        }
    });

    let (request, response_tx) =
        receive_execute_batch(&mut envoy_rx).await;
    assert_eq!(request.statements.len(), 2);
    assert_eq!(request.statements[0].sql, "insert-one");
    assert_eq!(request.statements[1].sql, "insert-two");
    send_execute_batch_ok(response_tx, 2);

    let results = batch.await.unwrap().unwrap();
    assert_eq!(results.len(), 2);
    assert!(
        envoy_rx.try_recv().is_err(),
        "batch emitted an extra request"
    );
}

#[tokio::test]
async fn remote_execute_batch_rolls_back_after_statement_failure() {
    let (handle, mut envoy_rx) = test_envoy_handle();
    let db = SqliteDb::new_with_remote_sqlite(
        handle,
        "actor-a",
        None,
        Some(7),
        true,
        true,
    )
    .expect("test remote sqlite should be configured");

    let batch = tokio::spawn({
        let db = db.clone();
        async move {
            db.execute_batch(vec![SqliteBatchStatement {
                sql: "fails".to_owned(),
                params: None,
            }])
            .await
        }
    });

    let (request, failure) =
        receive_execute_batch(&mut envoy_rx).await;
    assert_eq!(request.statements.len(), 1);
    assert_eq!(request.statements[0].sql, "fails");
    failure
        .send(Err(anyhow::anyhow!("injected statement failure")))
        .expect("remote sqlite requester dropped response");

    let error = batch.await.unwrap().expect_err("batch should fail");
    assert!(
        format!("{error:#}").contains("injected statement failure")
    );
    assert!(
        envoy_rx.try_recv().is_err(),
        "batch emitted an extra request"
    );
}

#[tokio::test]
async fn remote_transactions_park_ordinary_work_and_unpark_in_order()
{
    let (handle, mut envoy_rx) = test_envoy_handle();
    let db = SqliteDb::new_with_remote_sqlite(
        handle,
        "actor-a",
        None,
        Some(7),
        true,
        true,
    )
    .expect("test remote sqlite should be configured");

    let begin = tokio::spawn({
        let db = db.clone();
        async move { db.begin_transaction(None).await }
    });
    respond_to_execute(&mut envoy_rx, "BEGIN").await;
    let transaction = begin.await.unwrap().unwrap();

    let ordinary = tokio::spawn({
        let db = db.clone();
        async move { db.execute("ordinary", None).await }
    });
    tokio::task::yield_now().await;
    assert!(
        envoy_rx.try_recv().is_err(),
        "ordinary work interleaved"
    );

    let inside = tokio::spawn({
        let transaction = transaction.clone();
        async move { transaction.execute("inside", None).await }
    });
    respond_to_execute(&mut envoy_rx, "inside").await;
    inside.await.unwrap().unwrap();

    let commit = tokio::spawn({
        let transaction = transaction.clone();
        async move { transaction.commit().await }
    });
    respond_to_execute(&mut envoy_rx, "COMMIT").await;
    commit.await.unwrap().unwrap();

    respond_to_execute(&mut envoy_rx, "ordinary").await;
    ordinary.await.unwrap().unwrap();
}

#[tokio::test]
async fn transaction_gate_serves_registered_waiters_in_fifo_order() {
    let (handle, mut envoy_rx) = test_envoy_handle();
    let db = SqliteDb::new_with_remote_sqlite(
        handle,
        "actor-a",
        None,
        Some(7),
        true,
        true,
    )
    .expect("test remote sqlite should be configured");
    let active_gate = Arc::clone(&db.transaction_coordinator.gate)
        .write_owned()
        .await;

    // Poll each future exactly once while the gate is held. This
    // makes the coordinator admission order deterministic without
    // assuming that promises crossing NAPI or Wasm are polled in
    // their JavaScript construction order.
    let second = db.begin_transaction_inner(
        "second".to_owned(),
        DEFAULT_TRANSACTION_TIMEOUT,
    );
    tokio::pin!(second);
    assert!(futures::poll!(second.as_mut()).is_pending());
    let third = db.begin_transaction_inner(
        "third".to_owned(),
        DEFAULT_TRANSACTION_TIMEOUT,
    );
    tokio::pin!(third);
    assert!(futures::poll!(third.as_mut()).is_pending());

    drop(active_gate);
    let (second, ()) = tokio::join!(
        second.as_mut(),
        respond_to_execute(&mut envoy_rx, "BEGIN")
    );
    let second = second.unwrap();
    assert!(
        tokio::time::timeout(
            Duration::from_millis(10),
            envoy_rx.recv()
        )
        .await
        .is_err(),
        "third waiter passed the still-active second transaction"
    );
    let (commit, ()) = tokio::join!(
        second.commit(),
        respond_to_execute(&mut envoy_rx, "COMMIT")
    );
    commit.unwrap();

    let (third, ()) = tokio::join!(
        third.as_mut(),
        respond_to_execute(&mut envoy_rx, "BEGIN")
    );
    let third = third.unwrap();
    let (rollback, ()) = tokio::join!(
        third.rollback(),
        respond_to_execute(&mut envoy_rx, "ROLLBACK")
    );
    rollback.unwrap();
}

#[tokio::test]
async fn committed_and_rolled_back_transaction_handles_are_terminal()
{
    let (handle, mut envoy_rx) = test_envoy_handle();
    let db = SqliteDb::new_with_remote_sqlite(
        handle,
        "actor-a",
        None,
        Some(7),
        true,
        true,
    )
    .expect("test remote sqlite should be configured");

    let begin = tokio::spawn({
        let db = db.clone();
        async move { db.begin_transaction(None).await }
    });
    respond_to_execute(&mut envoy_rx, "BEGIN").await;
    let committed = begin.await.unwrap().unwrap();
    let commit = tokio::spawn({
        let transaction = committed.clone();
        async move { transaction.commit().await }
    });
    respond_to_execute(&mut envoy_rx, "COMMIT").await;
    commit.await.unwrap().unwrap();
    assert!(
        committed
            .execute("must-not-run-after-commit", None)
            .await
            .unwrap_err()
            .downcast_ref::<TransactionTerminalError>()
            .is_some()
    );

    let begin = tokio::spawn({
        let db = db.clone();
        async move { db.begin_transaction(None).await }
    });
    respond_to_execute(&mut envoy_rx, "BEGIN").await;
    let rolled_back = begin.await.unwrap().unwrap();
    let rollback = tokio::spawn({
        let transaction = rolled_back.clone();
        async move { transaction.rollback().await }
    });
    respond_to_execute(&mut envoy_rx, "ROLLBACK").await;
    rollback.await.unwrap().unwrap();
    assert!(
        rolled_back
            .execute("must-not-run-after-rollback", None)
            .await
            .unwrap_err()
            .downcast_ref::<TransactionTerminalError>()
            .is_some()
    );
    assert!(envoy_rx.try_recv().is_err());
}

#[tokio::test]
async fn ordinary_remote_work_can_pipeline_without_a_transaction() {
    let (handle, mut envoy_rx) = test_envoy_handle();
    let db = SqliteDb::new_with_remote_sqlite(
        handle,
        "actor-a",
        None,
        Some(7),
        true,
        true,
    )
    .expect("test remote sqlite should be configured");

    let first = tokio::spawn({
        let db = db.clone();
        async move { db.execute("first", None).await }
    });
    let first_response =
        receive_execute(&mut envoy_rx, "first").await;
    let second = tokio::spawn({
        let db = db.clone();
        async move { db.execute("second", None).await }
    });
    let second_response =
        receive_execute(&mut envoy_rx, "second").await;

    send_execute_ok(second_response);
    send_execute_ok(first_response);
    first.await.unwrap().unwrap();
    second.await.unwrap().unwrap();
}

#[tokio::test]
async fn expired_remote_transaction_rolls_back_and_rejects_parked_work()
 {
    let (handle, mut envoy_rx) = test_envoy_handle();
    let db = SqliteDb::new_with_remote_sqlite(
        handle,
        "actor-a",
        None,
        Some(7),
        true,
        true,
    )
    .expect("test remote sqlite should be configured");

    let begin = tokio::spawn({
        let db = db.clone();
        async move {
            db.begin_transaction(Some(Duration::from_millis(25)))
                .await
        }
    });
    respond_to_execute(&mut envoy_rx, "BEGIN").await;
    let transaction = begin.await.unwrap().unwrap();
    let parked = tokio::spawn({
        let db = db.clone();
        async move { db.execute("must-not-run", None).await }
    });

    respond_to_execute(&mut envoy_rx, "ROLLBACK").await;
    let parked_error = parked.await.unwrap().unwrap_err();
    assert!(
        parked_error
            .downcast_ref::<TransactionExpiredError>()
            .is_some(),
        "parked operation should fail with expiry: {parked_error:#}"
    );
    assert!(
        envoy_rx.try_recv().is_err(),
        "parked SQL executed after expiry"
    );
    let terminal_error = transaction
        .execute("also-must-not-run", None)
        .await
        .unwrap_err();
    assert!(
        terminal_error
            .downcast_ref::<TransactionExpiredError>()
            .is_some()
    );

    let fresh = tokio::spawn({
        let db = db.clone();
        async move { db.execute("fresh", None).await }
    });
    respond_to_execute(&mut envoy_rx, "fresh").await;
    fresh.await.unwrap().unwrap();
}

#[tokio::test]
async fn deadline_waits_for_in_flight_transaction_work_before_rollback()
 {
    let (handle, mut envoy_rx) = test_envoy_handle();
    let db = SqliteDb::new_with_remote_sqlite(
        handle,
        "actor-a",
        None,
        Some(7),
        true,
        true,
    )
    .expect("test remote sqlite should be configured");

    let begin = tokio::spawn({
        let db = db.clone();
        async move {
            db.begin_transaction(Some(Duration::from_millis(10)))
                .await
        }
    });
    respond_to_execute(&mut envoy_rx, "BEGIN").await;
    let transaction = begin.await.unwrap().unwrap();
    let inside = tokio::spawn({
        let transaction = transaction.clone();
        async move {
            transaction.execute("inside-at-deadline", None).await
        }
    });
    let inside_response =
        receive_execute(&mut envoy_rx, "inside-at-deadline").await;
    let queued = tokio::spawn({
        let transaction = transaction.clone();
        async move {
            transaction.execute("queued-before-deadline", None).await
        }
    });
    wait_for_coordinator_state(
        &db,
        "deadline must mark in-flight SQL expired",
        |state| {
            state
                .active
                .as_ref()
                .is_some_and(|active| active.expiring)
        },
    )
    .await;
    assert!(
        envoy_rx.try_recv().is_err(),
        "rollback raced in-flight SQL"
    );
    let during_cleanup = tokio::spawn({
        let db = db.clone();
        async move { db.execute("ordinary-during-expiry", None).await }
    });
    tokio::task::yield_now().await;

    send_execute_ok(inside_response);
    inside.await.unwrap().unwrap();
    respond_to_execute(&mut envoy_rx, "ROLLBACK").await;
    let queued_error = queued.await.unwrap().unwrap_err();
    assert!(
        queued_error
            .downcast_ref::<TransactionExpiredError>()
            .is_some()
    );
    assert!(
        envoy_rx.try_recv().is_err(),
        "queued SQL ran after deadline"
    );
    assert!(
        during_cleanup
            .await
            .unwrap()
            .unwrap_err()
            .downcast_ref::<TransactionExpiredError>()
            .is_some()
    );
    assert!(
        transaction
            .execute("must-not-run", None)
            .await
            .unwrap_err()
            .downcast_ref::<TransactionExpiredError>()
            .is_some()
    );
}

#[tokio::test]
async fn commit_that_owns_operation_lock_beats_deadline_without_poisoning_parked_work()
 {
    let (handle, mut envoy_rx) = test_envoy_handle();
    let db = SqliteDb::new_with_remote_sqlite(
        handle,
        "actor-a",
        None,
        Some(7),
        true,
        true,
    )
    .expect("test remote sqlite should be configured");

    let begin = tokio::spawn({
        let db = db.clone();
        async move {
            db.begin_transaction(Some(Duration::from_millis(10)))
                .await
        }
    });
    respond_to_execute(&mut envoy_rx, "BEGIN").await;
    let transaction = begin.await.unwrap().unwrap();
    let commit =
        tokio::spawn(async move { transaction.commit().await });
    let commit_response =
        receive_execute(&mut envoy_rx, "COMMIT").await;
    let parked = tokio::spawn({
        let db = db.clone();
        async move { db.execute("ordinary-after-commit", None).await }
    });

    wait_for_coordinator_state(
        &db,
        "deadline must enter expiry",
        |state| {
            state
                .active
                .as_ref()
                .is_some_and(|active| active.expiring)
        },
    )
    .await;
    assert!(
        envoy_rx.try_recv().is_err(),
        "deadline raced in-flight commit"
    );
    send_execute_ok(commit_response);
    commit.await.unwrap().unwrap();
    respond_to_execute(&mut envoy_rx, "ordinary-after-commit").await;
    parked.await.unwrap().unwrap();
    assert!(
        envoy_rx.try_recv().is_err(),
        "deadline sent rollback after commit"
    );
}

#[tokio::test]
async fn cancelled_begin_still_installs_and_expires_the_transaction()
{
    let (handle, mut envoy_rx) = test_envoy_handle();
    let db = SqliteDb::new_with_remote_sqlite(
        handle,
        "actor-a",
        None,
        Some(7),
        true,
        true,
    )
    .expect("test remote sqlite should be configured");

    let begin = tokio::spawn({
        let db = db.clone();
        async move {
            db.begin_transaction_with_key(
                "cancelled-begin",
                Some(Duration::from_millis(25)),
            )
            .await
        }
    });
    let begin_response =
        receive_execute(&mut envoy_rx, "BEGIN").await;
    begin.abort();
    assert!(
        matches!(begin.await, Err(error) if error.is_cancelled())
    );
    send_execute_ok(begin_response);

    respond_to_execute(&mut envoy_rx, "ROLLBACK").await;
    let fresh = tokio::spawn({
        let db = db.clone();
        async move {
            db.execute("fresh-after-cancelled-begin", None).await
        }
    });
    respond_to_execute(&mut envoy_rx, "fresh-after-cancelled-begin")
        .await;
    fresh.await.unwrap().unwrap();
}

#[tokio::test]
async fn cancelled_commit_still_releases_the_transaction() {
    let (handle, mut envoy_rx) = test_envoy_handle();
    let db = SqliteDb::new_with_remote_sqlite(
        handle,
        "actor-a",
        None,
        Some(7),
        true,
        true,
    )
    .expect("test remote sqlite should be configured");

    let begin = tokio::spawn({
        let db = db.clone();
        async move { db.begin_transaction(None).await }
    });
    respond_to_execute(&mut envoy_rx, "BEGIN").await;
    let transaction = begin.await.unwrap().unwrap();

    let commit = tokio::spawn({
        let transaction = transaction.clone();
        async move { transaction.commit().await }
    });
    let commit_response =
        receive_execute(&mut envoy_rx, "COMMIT").await;
    commit.abort();
    assert!(commit.await.unwrap_err().is_cancelled());
    send_execute_ok(commit_response);

    let fresh = tokio::spawn({
        let db = db.clone();
        async move {
            db.execute("fresh-after-cancelled-commit", None).await
        }
    });
    respond_to_execute(&mut envoy_rx, "fresh-after-cancelled-commit")
        .await;
    fresh.await.unwrap().unwrap();
}

#[tokio::test]
async fn cancelled_transaction_operation_settles_before_expiry_rollback()
 {
    let (handle, mut envoy_rx) = test_envoy_handle();
    let db = SqliteDb::new_with_remote_sqlite(
        handle,
        "actor-a",
        None,
        Some(7),
        true,
        true,
    )
    .expect("test remote sqlite should be configured");

    let begin = tokio::spawn({
        let db = db.clone();
        async move { db.begin_transaction(None).await }
    });
    respond_to_execute(&mut envoy_rx, "BEGIN").await;
    let transaction = begin.await.unwrap().unwrap();

    let operation = tokio::spawn({
        let transaction = transaction.clone();
        async move { transaction.execute("cancelled-inside", None).await }
    });
    let operation_response =
        receive_execute(&mut envoy_rx, "cancelled-inside").await;
    operation.abort();
    assert!(operation.await.unwrap_err().is_cancelled());

    let expiry = tokio::spawn({
        let transaction = transaction.clone();
        async move { transaction.expire().await }
    });
    wait_for_coordinator_state(
        &db,
        "explicit expiry must mark the transaction",
        |state| {
            state
                .active
                .as_ref()
                .is_some_and(|active| active.expiring)
        },
    )
    .await;
    assert!(
        envoy_rx.try_recv().is_err(),
        "rollback raced cancelled SQL"
    );

    send_execute_ok(operation_response);
    respond_to_execute(&mut envoy_rx, "ROLLBACK").await;
    expiry.await.unwrap().unwrap();
    assert!(
        transaction
            .execute("must-not-run", None)
            .await
            .unwrap_err()
            .downcast_ref::<TransactionExpiredError>()
            .is_some()
    );
}

#[tokio::test]
async fn shutdown_during_begin_rolls_back_without_orphaning_the_gate()
{
    let (handle, mut envoy_rx) = test_envoy_handle();
    let db = SqliteDb::new_with_remote_sqlite(
        handle,
        "actor-a",
        None,
        Some(7),
        true,
        true,
    )
    .expect("test remote sqlite should be configured");

    let begin = tokio::spawn({
        let db = db.clone();
        async move { db.begin_transaction(None).await }
    });
    let begin_response =
        receive_execute(&mut envoy_rx, "BEGIN").await;
    let close = tokio::spawn({
        let db = db.clone();
        async move { db.close().await }
    });
    wait_for_coordinator_state(
        &db,
        "shutdown must close admission before BEGIN settles",
        |state| state.closed,
    )
    .await;
    send_execute_ok(begin_response);
    respond_to_execute(&mut envoy_rx, "ROLLBACK").await;

    let Err(error) = begin.await.unwrap() else {
        panic!("begin must fail when shutdown wins");
    };
    assert!(
        error
            .downcast_ref::<TransactionCoordinatorClosedError>()
            .is_some()
    );
    close.await.unwrap().unwrap();
    assert!(
        db.execute("must-not-run", None)
            .await
            .unwrap_err()
            .downcast_ref::<TransactionCoordinatorClosedError>()
            .is_some()
    );
}

#[tokio::test]
async fn shutdown_rechecks_owner_after_concurrent_commit() {
    let (handle, mut envoy_rx) = test_envoy_handle();
    let db = SqliteDb::new_with_remote_sqlite(
        handle,
        "actor-a",
        None,
        Some(7),
        true,
        true,
    )
    .expect("test remote sqlite should be configured");

    let begin = tokio::spawn({
        let db = db.clone();
        async move { db.begin_transaction(None).await }
    });
    respond_to_execute(&mut envoy_rx, "BEGIN").await;
    let transaction = begin.await.unwrap().unwrap();
    let commit =
        tokio::spawn(async move { transaction.commit().await });
    let commit_response =
        receive_execute(&mut envoy_rx, "COMMIT").await;
    let close = tokio::spawn({
        let db = db.clone();
        async move { db.close().await }
    });
    wait_for_coordinator_state(
        &db,
        "shutdown must snapshot the active owner",
        |state| state.closed,
    )
    .await;
    send_execute_ok(commit_response);
    commit.await.unwrap().unwrap();
    close.await.unwrap().unwrap();
    assert!(
        envoy_rx.try_recv().is_err(),
        "shutdown sent a second rollback"
    );
}

#[tokio::test]
async fn failed_begin_releases_owner_and_allows_key_retry() {
    let (handle, mut envoy_rx) = test_envoy_handle();
    let db = SqliteDb::new_with_remote_sqlite(
        handle,
        "actor-a",
        None,
        Some(7),
        true,
        true,
    )
    .expect("test remote sqlite should be configured");

    let failed_begin = tokio::spawn({
        let db = db.clone();
        async move {
            db.begin_transaction_with_key("retry-key", None).await
        }
    });
    receive_execute(&mut envoy_rx, "BEGIN")
        .await
        .send(Err(anyhow::anyhow!("begin failed")))
        .expect("failed begin requester dropped response");
    assert!(failed_begin.await.unwrap().is_err());

    let retry = tokio::spawn({
        let db = db.clone();
        async move {
            db.begin_transaction_with_key("retry-key", None).await
        }
    });
    respond_to_execute(&mut envoy_rx, "BEGIN").await;
    let transaction = retry.await.unwrap().unwrap();
    let rollback =
        tokio::spawn(async move { transaction.rollback().await });
    respond_to_execute(&mut envoy_rx, "ROLLBACK").await;
    rollback.await.unwrap().unwrap();
}

#[tokio::test]
async fn disconnect_during_begin_never_publishes_a_transaction_handle()
 {
    let (handle, mut envoy_rx) = test_envoy_handle();
    let db = SqliteDb::new_with_remote_sqlite(
        handle,
        "actor-a",
        None,
        Some(7),
        true,
        true,
    )
    .expect("test remote sqlite should be configured");
    let begin = tokio::spawn({
        let db = db.clone();
        async move { db.begin_transaction(None).await }
    });
    receive_execute(&mut envoy_rx, "BEGIN")
        .await
        .send(Err(anyhow::anyhow!(
            RemoteSqliteIndeterminateResultError {
                operation: "execute",
            }
        )))
        .expect("begin requester dropped response");
    let error = begin
        .await
        .unwrap()
        .err()
        .expect("indeterminate BEGIN must fail");
    assert_eq!(
        ActorError::extract(&error).code(),
        "remote_indeterminate_result"
    );
    assert!(
        db.transaction_coordinator
            .state
            .lock()
            .await
            .active
            .is_none()
    );
}

#[tokio::test]
async fn disconnect_during_statement_terminalizes_the_transaction() {
    let (handle, mut envoy_rx) = test_envoy_handle();
    let db = SqliteDb::new_with_remote_sqlite(
        handle,
        "actor-a",
        None,
        Some(7),
        true,
        true,
    )
    .expect("test remote sqlite should be configured");
    let begin = tokio::spawn({
        let db = db.clone();
        async move { db.begin_transaction(None).await }
    });
    respond_to_execute(&mut envoy_rx, "BEGIN").await;
    let transaction = begin.await.unwrap().unwrap();
    let statement = tokio::spawn({
        let transaction = transaction.clone();
        async move { transaction.execute("in flight", None).await }
    });
    receive_execute(&mut envoy_rx, "in flight")
        .await
        .send(Err(anyhow::anyhow!(
            RemoteSqliteIndeterminateResultError {
                operation: "execute",
            }
        )))
        .expect("statement requester dropped response");
    let error = statement.await.unwrap().unwrap_err();
    assert_eq!(
        ActorError::extract(&error).code(),
        "remote_indeterminate_result"
    );
    assert!(
        transaction
            .execute("must not execute", None)
            .await
            .unwrap_err()
            .downcast_ref::<TransactionConnectionLostError>()
            .is_some()
    );
    assert!(
        envoy_rx.try_recv().is_err(),
        "stale statement crossed session"
    );
}

#[tokio::test]
async fn disconnect_during_commit_stays_indeterminate_and_releases_waiters()
 {
    let (handle, mut envoy_rx) = test_envoy_handle();
    let db = SqliteDb::new_with_remote_sqlite(
        handle,
        "actor-a",
        None,
        Some(7),
        true,
        true,
    )
    .expect("test remote sqlite should be configured");
    let begin = tokio::spawn({
        let db = db.clone();
        async move { db.begin_transaction(None).await }
    });
    respond_to_execute(&mut envoy_rx, "BEGIN").await;
    let transaction = begin.await.unwrap().unwrap();
    let commit =
        tokio::spawn(async move { transaction.commit().await });
    receive_execute(&mut envoy_rx, "COMMIT")
        .await
        .send(Err(anyhow::anyhow!(
            RemoteSqliteIndeterminateResultError {
                operation: "execute",
            }
        )))
        .expect("commit requester dropped response");
    let error = commit.await.unwrap().unwrap_err();
    assert_eq!(
        ActorError::extract(&error).code(),
        "remote_indeterminate_result"
    );

    let fresh = tokio::spawn({
        let db = db.clone();
        async move {
            db.execute("fresh after indeterminate commit", None).await
        }
    });
    respond_to_execute(
        &mut envoy_rx,
        "fresh after indeterminate commit",
    )
    .await;
    fresh.await.unwrap().unwrap();
}

#[tokio::test]
async fn failed_commit_rolls_back_and_releases_the_transaction() {
    let (handle, mut envoy_rx) = test_envoy_handle();
    let db = SqliteDb::new_with_remote_sqlite(
        handle,
        "actor-a",
        None,
        Some(7),
        true,
        true,
    )
    .expect("test remote sqlite should be configured");

    let begin = tokio::spawn({
        let db = db.clone();
        async move { db.begin_transaction(None).await }
    });
    respond_to_execute(&mut envoy_rx, "BEGIN").await;
    let transaction = begin.await.unwrap().unwrap();

    let commit =
        tokio::spawn(async move { transaction.commit().await });
    receive_execute(&mut envoy_rx, "COMMIT")
        .await
        .send(Err(anyhow::anyhow!("commit failed")))
        .expect("failed commit requester dropped response");
    respond_to_execute(&mut envoy_rx, "ROLLBACK").await;
    assert!(commit.await.unwrap().is_err());

    let fresh = tokio::spawn({
        let db = db.clone();
        async move { db.execute("fresh-after-failed-commit", None).await }
    });
    respond_to_execute(&mut envoy_rx, "fresh-after-failed-commit")
        .await;
    fresh.await.unwrap().unwrap();
}

#[tokio::test]
async fn failed_commit_accepts_sqlite_auto_rollback_as_cleanup_success()
 {
    let (handle, mut envoy_rx) = test_envoy_handle();
    let db = SqliteDb::new_with_remote_sqlite(
        handle,
        "actor-a",
        None,
        Some(7),
        true,
        true,
    )
    .expect("test remote sqlite should be configured");

    let begin = tokio::spawn({
        let db = db.clone();
        async move { db.begin_transaction(None).await }
    });
    respond_to_execute(&mut envoy_rx, "BEGIN").await;
    let transaction = begin.await.unwrap().unwrap();
    let commit =
        tokio::spawn(async move { transaction.commit().await });
    receive_execute(&mut envoy_rx, "COMMIT")
        .await
        .send(Err(anyhow::anyhow!(
            "commit failed after SQLite auto-rollback"
        )))
        .expect("failed commit requester dropped response");
    receive_execute(&mut envoy_rx, "ROLLBACK")
        .await
        .send(Err(anyhow::anyhow!(
            "cannot rollback - no transaction is active"
        )))
        .expect("rollback requester dropped response");
    assert!(
        commit.await.unwrap().is_err(),
        "commit error remains primary"
    );

    let fresh = tokio::spawn({
        let db = db.clone();
        async move { db.execute("fresh-after-auto-rollback", None).await }
    });
    respond_to_execute(&mut envoy_rx, "fresh-after-auto-rollback")
        .await;
    fresh.await.unwrap().unwrap();
}

#[tokio::test]
async fn failed_rollback_closes_the_coordinator() {
    let (handle, mut envoy_rx) = test_envoy_handle();
    let db = SqliteDb::new_with_remote_sqlite(
        handle,
        "actor-a",
        None,
        Some(7),
        true,
        true,
    )
    .expect("test remote sqlite should be configured");

    let begin = tokio::spawn({
        let db = db.clone();
        async move { db.begin_transaction(None).await }
    });
    respond_to_execute(&mut envoy_rx, "BEGIN").await;
    let transaction = begin.await.unwrap().unwrap();
    let rollback =
        tokio::spawn(async move { transaction.rollback().await });
    receive_execute(&mut envoy_rx, "ROLLBACK")
        .await
        .send(Err(anyhow::anyhow!("rollback failed")))
        .expect("failed rollback requester dropped response");
    assert!(rollback.await.unwrap().is_err());
    let error = db.execute("must-not-run", None).await.unwrap_err();
    assert!(
        error
            .downcast_ref::<TransactionCoordinatorClosedError>()
            .is_some()
    );
    assert!(
        db.try_transaction_admission()
            .unwrap_err()
            .downcast_ref::<TransactionCoordinatorClosedError>()
            .is_some()
    );
}

#[tokio::test]
async fn close_rolls_back_active_transaction_and_rejects_later_work()
{
    let (handle, mut envoy_rx) = test_envoy_handle();
    let db = SqliteDb::new_with_remote_sqlite(
        handle,
        "actor-a",
        None,
        Some(7),
        true,
        true,
    )
    .expect("test remote sqlite should be configured");

    let begin = tokio::spawn({
        let db = db.clone();
        async move { db.begin_transaction(None).await }
    });
    respond_to_execute(&mut envoy_rx, "BEGIN").await;
    let _transaction = begin.await.unwrap().unwrap();

    let close = tokio::spawn({
        let db = db.clone();
        async move { db.close().await }
    });
    respond_to_execute(&mut envoy_rx, "ROLLBACK").await;
    close.await.unwrap().unwrap();
    let error = db.execute("must-not-run", None).await.unwrap_err();
    assert!(
        error
            .downcast_ref::<TransactionCoordinatorClosedError>()
            .is_some()
    );
    assert!(envoy_rx.try_recv().is_err());
}

#[tokio::test]
async fn cancelled_close_still_finishes_rollback_and_release() {
    let (handle, mut envoy_rx) = test_envoy_handle();
    let db = SqliteDb::new_with_remote_sqlite(
        handle,
        "actor-a",
        None,
        Some(7),
        true,
        true,
    )
    .expect("test remote sqlite should be configured");
    let begin = tokio::spawn({
        let db = db.clone();
        async move { db.begin_transaction(None).await }
    });
    respond_to_execute(&mut envoy_rx, "BEGIN").await;
    let _transaction = begin.await.unwrap().unwrap();

    let close = tokio::spawn({
        let db = db.clone();
        async move { db.close().await }
    });
    let rollback_response =
        receive_execute(&mut envoy_rx, "ROLLBACK").await;
    close.abort();
    assert!(close.await.unwrap_err().is_cancelled());
    send_execute_ok(rollback_response);

    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if db
                .transaction_coordinator
                .state
                .lock()
                .await
                .active
                .is_none()
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("detached close must finish");
    assert!(
        db.execute("must-not-run", None)
            .await
            .unwrap_err()
            .downcast_ref::<TransactionCoordinatorClosedError>()
            .is_some()
    );
}

#[test]
fn transaction_deadline_defaults_to_sixty_seconds() {
    assert_eq!(DEFAULT_TRANSACTION_TIMEOUT, Duration::from_secs(60));
}

#[test]
fn terminal_transaction_state_is_bounded() {
    let mut state = TransactionCoordinatorState {
        active: None,
        terminal: BTreeMap::new(),
        terminal_order: std::collections::VecDeque::new(),
        poisoned: BTreeMap::new(),
        poisoned_order: std::collections::VecDeque::new(),
        last_expired_timeout: None,
        closed: false,
    };
    insert_terminal_state(
        &mut state,
        "expired-forever".to_owned(),
        TransactionTerminalState::Expired(Duration::from_secs(60)),
    );
    for index in 0..=TRANSACTION_TERMINAL_CAPACITY {
        insert_terminal_state(
            &mut state,
            format!("transaction-{index}"),
            TransactionTerminalState::Committed,
        );
    }
    assert_eq!(state.terminal.len(), TRANSACTION_TERMINAL_CAPACITY);
    assert!(!state.terminal.contains_key("transaction-0"));
    assert!(state.terminal.contains_key(&format!(
        "transaction-{TRANSACTION_TERMINAL_CAPACITY}"
    )));
    assert!(matches!(
        state.poisoned.get("expired-forever"),
        Some(expired) if expired.timeout == Duration::from_secs(60)
    ));
}

#[tokio::test]
async fn admission_reports_queue_full_and_closed_distinctly() {
    let (handle, envoy_rx) = test_envoy_handle();
    drop(envoy_rx);
    let db = SqliteDb::new_with_remote_sqlite(
        handle,
        "actor-a",
        None,
        Some(7),
        true,
        true,
    )
    .expect("test remote sqlite should be configured");
    let permits = (0..TRANSACTION_COORDINATOR_QUEUE_CAPACITY)
        .map(|_| db.try_transaction_admission().unwrap())
        .collect::<Vec<_>>();
    let full = db.try_transaction_admission().unwrap_err();
    assert!(
        full.downcast_ref::<TransactionQueueFullError>().is_some()
    );
    drop(permits);
    db.transaction_coordinator.admission.close();
    let closed = db.try_transaction_admission().unwrap_err();
    assert!(
        closed
            .downcast_ref::<TransactionCoordinatorClosedError>()
            .is_some()
    );
}

#[tokio::test]
async fn mismatched_release_does_not_drop_the_active_transaction() {
    let (handle, mut envoy_rx) = test_envoy_handle();
    let db = SqliteDb::new_with_remote_sqlite(
        handle,
        "actor-a",
        None,
        Some(7),
        true,
        true,
    )
    .expect("test remote sqlite should be configured");
    let begin = tokio::spawn({
        let db = db.clone();
        async move { db.begin_transaction_with_key("owner", None).await }
    });
    respond_to_execute(&mut envoy_rx, "BEGIN").await;
    let transaction = begin.await.unwrap().unwrap();
    db.release_transaction(
        "not-owner",
        TransactionTerminalState::Committed,
        false,
    )
    .await;

    let inside = tokio::spawn({
        let transaction = transaction.clone();
        async move { transaction.execute("still-owned", None).await }
    });
    respond_to_execute(&mut envoy_rx, "still-owned").await;
    inside.await.unwrap().unwrap();
    let rollback =
        tokio::spawn(async move { transaction.rollback().await });
    respond_to_execute(&mut envoy_rx, "ROLLBACK").await;
    rollback.await.unwrap().unwrap();
}

#[tokio::test]
async fn remote_disconnect_terminalizes_transaction_and_unparks_new_work()
 {
    let (handle, mut envoy_rx, shared) =
        test_envoy_handle_with_shared();
    let db = SqliteDb::new_with_remote_sqlite(
        handle,
        "actor-a",
        None,
        Some(7),
        true,
        true,
    )
    .expect("test remote sqlite should be configured");
    let begin = tokio::spawn({
        let db = db.clone();
        async move {
            db.begin_transaction_with_key("session-owned", None).await
        }
    });
    respond_to_execute(&mut envoy_rx, "BEGIN").await;
    let transaction = begin.await.unwrap().unwrap();

    let queued = tokio::spawn({
        let db = db.clone();
        async move { db.execute("after reconnect", None).await }
    });
    assert!(
        envoy_rx.try_recv().is_err(),
        "ordinary SQL must remain parked"
    );

    // Disconnect is the rollback boundary for remote SQLite:
    // pegboard-envoy drops the connection-owned database handle. The
    // watch signal must make the client transaction terminal even
    // when no transaction statement was in flight, which is the
    // fast-reconnect edge case that otherwise autocommits a
    // stale transaction's next statement.
    shared.connection_session.store(0, Ordering::Release);
    shared.connection_session_tx.send_replace(0);
    wait_for_coordinator_state(
        &db,
        "disconnect should release transaction",
        |state| state.active.is_none(),
    )
    .await;

    shared.connection_session.store(2, Ordering::Release);
    shared.connection_session_tx.send_replace(2);
    let stale_error = transaction
        .execute("must not execute", None)
        .await
        .expect_err("stale transaction handle must be terminal");
    assert!(
        stale_error
            .downcast_ref::<TransactionConnectionLostError>()
            .is_some()
    );

    respond_to_execute(&mut envoy_rx, "after reconnect").await;
    queued.await.unwrap().unwrap();
}
