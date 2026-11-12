// This file was changed for Hamlet. See NOTICE in this crate.
#[expect(dead_code, reason = "part of the protocol's header set")]
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const USER_AGENT_VALUE: &str =
    concat!("ActorClient-Rust/", env!("CARGO_PKG_VERSION"));

// Headers
#[expect(dead_code, reason = "part of the protocol's header set")]
pub const HEADER_ACTOR_QUERY: &str = "x-hamlet-actor-query";
pub const HEADER_ENCODING: &str = "x-hamlet-actor-encoding";
pub const HEADER_CONN_PARAMS: &str = "x-hamlet-actor-conn-params";
#[expect(dead_code, reason = "part of the protocol's header set")]
pub const HEADER_ACTOR_ID: &str = "x-hamlet-actor-actor";
#[expect(dead_code, reason = "part of the protocol's header set")]
pub const HEADER_CONN_ID: &str = "x-hamlet-actor-conn";
#[expect(dead_code, reason = "part of the protocol's header set")]
pub const HEADER_CONN_TOKEN: &str = "x-hamlet-actor-conn-token";

// Gateway headers
pub const HEADER_HAMLET_ACTOR_TARGET: &str = "x-hamlet-actor-target";
pub const HEADER_HAMLET_ACTOR_ACTOR: &str = "x-hamlet-actor-actor";
pub const HEADER_HAMLET_ACTOR_TOKEN: &str = "x-hamlet-actor-token";
pub const HEADER_HAMLET_ACTOR_NAMESPACE: &str =
    "x-hamlet-actor-namespace";

// Paths
pub const PATH_CONNECT_WEBSOCKET: &str = "/connect";
pub const PATH_WEBSOCKET_PREFIX: &str = "/websocket/";

pub type RawWebSocket = tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
>;

// WebSocket protocol prefixes
pub const WS_PROTOCOL_STANDARD: &str = "hamlet";
pub const WS_PROTOCOL_TARGET: &str = "actor_target.";
pub const WS_PROTOCOL_ACTOR: &str = "actor_actor.";
pub const WS_PROTOCOL_ENCODING: &str = "actor_encoding.";
pub const WS_PROTOCOL_CONN_PARAMS: &str = "actor_conn_params.";
pub const WS_PROTOCOL_CONN_ID: &str = "actor_conn.";
pub const WS_PROTOCOL_CONN_TOKEN: &str = "actor_conn_token.";
pub const WS_PROTOCOL_TOKEN: &str = "actor_token.";

#[derive(Debug, Clone, Copy)]
pub enum TransportKind {
    WebSocket,
    Sse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EncodingKind {
    Json,
    Cbor,
    #[default]
    Bare,
}

impl EncodingKind {
    pub fn as_str(&self) -> &str {
        match self {
            EncodingKind::Json => "json",
            EncodingKind::Cbor => "cbor",
            EncodingKind::Bare => "bare",
        }
    }
}

impl std::fmt::Display for EncodingKind {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// Max size of each entry is 128 bytes
pub type ActorKey = Vec<String>;
