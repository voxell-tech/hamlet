// This file was changed for Hamlet. See NOTICE in this crate.
//! What the core needs from the host that runs it: the [`ActorHost`]
//! trait, [`RunningActor`] to start, call and stop one actor's task,
//! and the plain types the core took from the engine client. Each
//! module below mirrors the path the core imports from.

mod running;

use std::sync::Arc;

use anyhow::Result;
use hamlet_actor_wire::shared_types::actor_key;
use tokio::sync::{oneshot, watch};

pub use self::running::{RunningActor, StartRequest};
use self::tunnel::HibernatingWebSocketMetadata;
use crate::async_counter::AsyncCounter;
use crate::runtime::RuntimeBoxFuture;
use crate::sqlite::{
    BindParam, ExecuteResult, QueryResult, SqliteBatchStatement,
};
use crate::types::{ActorKey, ActorKeySegment};

/// What the core holds to reach its host.
pub type HostHandle = Arc<dyn ActorHost>;

/// The host that runs an actor: it stores the actor's SQLite file,
/// keeps its next alarm, and stops it when it asks to sleep or to be
/// destroyed. The core calls these methods; a host implements them.
///
/// The methods with a default serve what a local host does not have:
/// HTTP request counting, hibernating WebSockets and the key-value
/// store from before SQLite.
pub trait ActorHost: Send + Sync + 'static {
    fn endpoint(&self) -> &str;

    fn token(&self) -> Option<&str> {
        None
    }

    fn namespace(&self) -> &str;

    fn pool_name(&self) -> &str;

    /// The current connection session to the actor's database. The
    /// transaction code compares sessions to detect a lost
    /// connection.
    fn connection_session(&self) -> Option<u64>;

    /// Follows the connection session. The sender must live as long
    /// as the host: a closed channel reads as a lost connection.
    fn subscribe_connection_session(&self) -> watch::Receiver<u64>;

    /// The actor asks to sleep.
    fn sleep_actor(
        &self,
        actor_id: String,
        generation: Option<u32>,
        error: Option<String>,
    );

    /// The actor asks to be destroyed.
    fn stop_actor(&self, actor_id: String, generation: Option<u32>);

    /// Stores the actor's next alarm durably, then sends on `ack_tx`.
    /// `None` clears it.
    fn set_alarm_with_ack(
        &self,
        actor_id: String,
        alarm_ts: Option<i64>,
        generation: Option<u32>,
        ack_tx: Option<oneshot::Sender<()>>,
    );

    fn sqlite_exec(
        &self,
        actor_id: String,
        sql: String,
    ) -> RuntimeBoxFuture<Result<QueryResult>>;

    fn sqlite_execute(
        &self,
        actor_id: String,
        sql: String,
        params: Option<Vec<BindParam>>,
    ) -> RuntimeBoxFuture<Result<ExecuteResult>>;

    /// Runs the statements as one transaction.
    fn sqlite_execute_batch(
        &self,
        actor_id: String,
        statements: Vec<SqliteBatchStatement>,
    ) -> RuntimeBoxFuture<Result<Vec<ExecuteResult>>>;

    /// A transaction on the actor's database committed: a
    /// transaction's commit, which state transactions use, or a
    /// batch. A statement that commits on its own, and a `COMMIT`
    /// sent as a statement, do not call it. It runs after the
    /// transaction released its gate, and must return at once.
    fn transaction_committed(&self, actor_id: &str);

    fn http_request_counter(
        &self,
        _actor_id: &str,
        _generation: Option<u32>,
    ) -> Option<Arc<AsyncCounter>> {
        None
    }

    fn hibernatable_connection_is_live(
        &self,
        _actor_id: &str,
        _generation: Option<u32>,
        _gateway_id: [u8; 4],
        _request_id: [u8; 4],
    ) -> bool {
        false
    }

    fn restore_hibernating_requests(
        &self,
        _actor_id: String,
        _entries: Vec<HibernatingWebSocketMetadata>,
    ) {
    }

    fn send_hibernatable_ws_message_ack(
        &self,
        _gateway_id: [u8; 4],
        _request_id: [u8; 4],
        _index: u16,
    ) {
    }

    fn kv_get(
        &self,
        _actor_id: String,
        keys: Vec<Vec<u8>>,
    ) -> RuntimeBoxFuture<Result<Vec<Option<Vec<u8>>>>> {
        Box::pin(async move { Ok(vec![None; keys.len()]) })
    }

    fn kv_put(
        &self,
        _actor_id: String,
        _entries: Vec<(Vec<u8>, Vec<u8>)>,
    ) -> RuntimeBoxFuture<Result<()>> {
        Box::pin(async { Ok(()) })
    }

    fn kv_list_prefix(
        &self,
        _actor_id: String,
        _prefix: Vec<u8>,
        _reverse: Option<bool>,
        _limit: Option<u64>,
    ) -> RuntimeBoxFuture<Result<Vec<(Vec<u8>, Vec<u8>)>>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn kv_list_range(
        &self,
        _actor_id: String,
        _start: Vec<u8>,
        _end: Vec<u8>,
        _exclusive: bool,
        _reverse: Option<bool>,
        _limit: Option<u64>,
    ) -> RuntimeBoxFuture<Result<Vec<(Vec<u8>, Vec<u8>)>>> {
        Box::pin(async { Ok(Vec::new()) })
    }
}

pub mod config {
    use std::collections::HashMap;

    pub const HTTP_BODY_STREAM_CHANNEL_CAPACITY: usize = 16;
    pub const HTTP_BODY_MAX_CHUNK_SIZE: usize = 64 * 1024;

    /// A request body that streams in. The local host never streams
    /// one.
    #[derive(Debug, Default)]
    pub struct HttpRequestBodyStream;

    impl HttpRequestBodyStream {
        pub async fn recv(
            &mut self,
        ) -> anyhow::Result<Option<Vec<u8>>> {
            Ok(None)
        }
    }

    #[derive(Debug, Default)]
    pub struct HttpRequestBodyQueue;

    pub struct HttpRequest {
        pub method: String,
        pub path: String,
        pub headers: HashMap<String, String>,
        pub body: Option<Vec<u8>>,
        pub body_stream: Option<HttpRequestBodyStream>,
    }

    pub enum ResponseChunk {
        Data { data: Vec<u8>, finish: bool },
        Error(String),
    }

    /// Sends on a WebSocket. The local host serves no sockets.
    #[derive(Clone)]
    pub struct WebSocketSender {
        _private: (),
    }

    impl WebSocketSender {
        pub fn send(&self, _data: Vec<u8>, _binary: bool) {}

        pub fn send_text(&self, _text: &str) {}

        pub async fn flush(&self) {}

        pub fn close(
            &self,
            _code: Option<u16>,
            _reason: Option<String>,
        ) {
        }
    }
}

pub mod tunnel {
    use std::collections::HashMap;

    pub struct HibernatingWebSocketMetadata {
        pub gateway_id: [u8; 4],
        pub request_id: [u8; 4],
        pub envoy_message_index: u16,
        pub actor_message_index: u16,
        pub path: String,
        pub headers: HashMap<String, String>,
    }
}

/// Remote SQLite errors the core downcasts to. A local file never
/// raises them.
pub mod utils {
    use std::fmt;

    #[derive(Debug)]
    pub struct RemoteSqliteIndeterminateResultError {
        pub operation: &'static str,
    }

    impl fmt::Display for RemoteSqliteIndeterminateResultError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(
                f,
                "sqlite {} result is indeterminate",
                self.operation
            )
        }
    }

    impl std::error::Error for RemoteSqliteIndeterminateResultError {}

    #[derive(Debug)]
    pub struct RemoteSqliteConnectionSessionLostError {
        pub expected: u64,
        pub current: Option<u64>,
    }

    impl fmt::Display for RemoteSqliteConnectionSessionLostError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(
                f,
                "sqlite session {} is no longer active",
                self.expected
            )
        }
    }

    impl std::error::Error for RemoteSqliteConnectionSessionLostError {}
}

/// An actor key from its slash-separated form.
pub(crate) fn actor_key_from_protocol(
    key: Option<String>,
) -> ActorKey {
    let Some(key) = key else {
        return Vec::new();
    };
    actor_key::slash::deserialize(&key)
        .into_iter()
        .map(ActorKeySegment::String)
        .collect()
}
