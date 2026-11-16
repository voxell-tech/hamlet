// This file was changed for Hamlet. See NOTICE in this package.
// NOTE: This is in a separate file from the router since it needs to be shared between the client & the server. If this was in the router file, the client would end up importing the *entire* actor router and tree shaking would not work.

// MARK: Paths
export const PATH_CONNECT = "/connect";
export const PATH_WEBSOCKET_BASE = "/websocket";
export const PATH_WEBSOCKET_PREFIX = "/websocket/";

// MARK: Headers
export const HEADER_ACTOR_QUERY = "x-hamlet-actor-query";

export const HEADER_ENCODING = "x-hamlet-actor-encoding";

// IMPORTANT: Params must be in headers or in an E2EE part of the request (i.e. NOT the URL or query string) in order to ensure that tokens can be securely passed in params.
export const HEADER_CONN_PARAMS = "x-hamlet-actor-conn-params";
export const HEADER_ORIGINAL_REQUEST_URL =
    "x-hamlet-actor-internal-original-request-url";

export const HEADER_ACTOR_ID = "x-hamlet-actor-actor";
export const HEADER_ACTOR_GENERATION = "x-hamlet-actor-actor-generation";
export const HEADER_ACTOR_KEY = "x-hamlet-actor-actor-key";

export const HEADER_HAMLET_ACTOR_TOKEN = "x-hamlet-actor-token";
export const HEADER_HAMLET_ACTOR_RAY_ID = "x-hamlet-actor-ray-id";
export const HEADER_TRACEPARENT = "traceparent";
export const HEADER_TRACESTATE = "tracestate";

// MARK: Manager Gateway Headers
export const HEADER_HAMLET_ACTOR_TARGET = "x-hamlet-actor-target";
export const HEADER_HAMLET_ACTOR_ACTOR = "x-hamlet-actor-actor";
export const HEADER_HAMLET_ACTOR_NAMESPACE = "x-hamlet-actor-namespace";
export const HEADER_HAMLET_ACTOR_SKIP_READY_WAIT =
    "x-hamlet-actor-skip-ready-wait";

// MARK: WebSocket Protocol Prefixes
/** Some servers (such as node-ws & Cloudflare) require explicitly match a certain WebSocket protocol. This gives us a static protocol to match against. */
export const WS_PROTOCOL_STANDARD = "hamlet";
export const WS_PROTOCOL_TARGET = "actor_target.";
export const WS_PROTOCOL_ACTOR = "actor_actor.";
export const WS_PROTOCOL_NAMESPACE = "actor_namespace.";
export const WS_PROTOCOL_ENCODING = "actor_encoding.";
export const WS_PROTOCOL_CONN_PARAMS = "actor_conn_params.";
export const WS_PROTOCOL_TOKEN = "actor_token.";
export const WS_PROTOCOL_SKIP_READY_WAIT = "actor_skip_ready_wait";
export const WS_PROTOCOL_TEST_ACK_HOOK = "actor_test_ack_hook.";

// MARK: WebSocket Inline Test Protocol Prefixes
export const WS_TEST_PROTOCOL_PATH = "test_path.";

/**
 * Headers that publics can send from public clients.
 *
 * Used for CORS.
 **/
export const ALLOWED_PUBLIC_HEADERS = [
    "Content-Type",
    "User-Agent",
    HEADER_ACTOR_QUERY,
    HEADER_ENCODING,
    HEADER_CONN_PARAMS,
    HEADER_ACTOR_ID,
    HEADER_ACTOR_GENERATION,
    HEADER_ACTOR_KEY,
    HEADER_HAMLET_ACTOR_TARGET,
    HEADER_HAMLET_ACTOR_ACTOR,
    HEADER_HAMLET_ACTOR_NAMESPACE,
    HEADER_HAMLET_ACTOR_TOKEN,
    HEADER_HAMLET_ACTOR_SKIP_READY_WAIT,
];
