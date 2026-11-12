//! The local host, which runs actors in this process on plain SQLite
//! files, and the plain types the core takes from its host. Each
//! module below mirrors the path the core imports from.

mod local;
mod plain_sqlite;

use hamlet_actor_shared_types::actor_key;

pub use self::local::{Host, HostHandle, now_ms, read_alarm};
pub use self::plain_sqlite::PlainSqlite;
use crate::types::{ActorKey, ActorKeySegment};

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
