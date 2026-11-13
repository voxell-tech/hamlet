// This file was changed for Hamlet. See NOTICE in this crate.
//! The HTTP helpers of an actor's own routes: route parsing, request
//! decoding, encodings and error bodies.

use std::collections::HashMap;
use std::io::Cursor;

use ::http::{self, StatusCode};
use anyhow::{Context, Result};
use hamlet_actor_error::{ActorError, ActorSpecifier};
use hamlet_actor_wire::client_protocol;
use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};
use vbare::Versioned;

use crate::actor::messages::Request;
use crate::error::ProtocolError;
use crate::host::config::HttpRequest;
use crate::{ActorContext, serde_metrics};

#[expect(
    dead_code,
    reason = "ported; nothing in Hamlet calls it yet"
)]
const HEADER_HAMLET_ACTOR_ACTOR: &str = "x-hamlet-actor-actor";
#[expect(
    dead_code,
    reason = "ported; nothing in Hamlet calls it yet"
)]
const HEADER_HAMLET_ACTOR_ACTOR_GENERATION: &str =
    "x-hamlet-actor-actor-generation";
#[expect(
    dead_code,
    reason = "ported; nothing in Hamlet calls it yet"
)]
const HEADER_HAMLET_ACTOR_ACTOR_KEY: &str =
    "x-hamlet-actor-actor-key";

#[expect(
    dead_code,
    reason = "ported; nothing in Hamlet calls it yet"
)]
#[derive(Debug, Deserialize)]
#[serde(default)]
struct HttpActionRequestJson {
    args: JsonValue,
}

impl Default for HttpActionRequestJson {
    fn default() -> Self {
        Self {
            args: JsonValue::Array(Vec::new()),
        }
    }
}

#[expect(
    dead_code,
    reason = "ported; nothing in Hamlet calls it yet"
)]
#[derive(Debug, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct HttpQueueSendRequestJson {
    body: JsonValue,
    wait: Option<bool>,
    timeout: Option<u64>,
}

impl Default for HttpQueueSendRequestJson {
    fn default() -> Self {
        Self {
            body: JsonValue::Null,
            wait: None,
            timeout: None,
        }
    }
}

/// Cancels a user request's token when the request finishes, unless a
/// streaming response body takes ownership of it.
///
/// Every terminal path must cancel the token. Foreign runtimes park a
/// task per request on it to deliver `abortSignal`, and a token that
/// is dropped without being cancelled strands that task and its JS
/// callback until the actor stops.
#[expect(
    dead_code,
    reason = "ported; nothing in Hamlet calls it yet"
)]
struct RequestCancellationGuard {
    token: Option<tokio_util::sync::CancellationToken>,
}

impl RequestCancellationGuard {
    #[expect(
        dead_code,
        reason = "ported; nothing in Hamlet calls it yet"
    )]
    fn new(token: tokio_util::sync::CancellationToken) -> Self {
        Self { token: Some(token) }
    }
}

impl Drop for RequestCancellationGuard {
    fn drop(&mut self) {
        if let Some(token) = self.token.take() {
            token.cancel();
        }
    }
}

#[expect(
    dead_code,
    reason = "ported; nothing in Hamlet calls it yet"
)]
enum RegistryHttpRoute {
    Framework(FrameworkHttpRoute),
    UserRawRequest,
}

impl RegistryHttpRoute {
    #[expect(
        dead_code,
        reason = "ported; nothing in Hamlet calls it yet"
    )]
    fn from_paths(
        original_path: &str,
        normalized_path: &str,
        handle_inspector_http_in_runtime: bool,
    ) -> Result<Self> {
        if let Some(stripped) = original_path.strip_prefix("/request")
            && (stripped.is_empty()
                || matches!(
                    stripped.as_bytes().first(),
                    Some(b'/') | Some(b'?')
                ))
        {
            return Ok(Self::UserRawRequest);
        }
        if handle_inspector_http_in_runtime
            && normalized_path.starts_with("/inspector/")
        {
            return Ok(Self::UserRawRequest);
        }

        if let Some(segment) =
            single_path_segment(normalized_path, "/action/")
        {
            return Ok(Self::Framework(FrameworkHttpRoute::Action(
                percent_decode_path_segment(segment)?,
            )));
        }
        if let Some(segment) =
            single_path_segment(normalized_path, "/queue/")
        {
            return Ok(Self::Framework(FrameworkHttpRoute::Queue(
                percent_decode_path_segment(segment)?,
            )));
        }

        match normalized_path {
            "/metadata" => {
                Ok(Self::Framework(FrameworkHttpRoute::Metadata))
            }
            "/health" => {
                Ok(Self::Framework(FrameworkHttpRoute::Health))
            }
            "/" => Ok(Self::Framework(FrameworkHttpRoute::Root)),
            _ => Ok(Self::Framework(FrameworkHttpRoute::NotFound)),
        }
    }
}

#[expect(
    dead_code,
    reason = "ported; nothing in Hamlet calls it yet"
)]
pub(crate) enum FrameworkHttpRoute {
    Action(String),
    Queue(String),
    Metadata,
    Health,
    Root,
    NotFound,
}

#[expect(
    dead_code,
    reason = "ported; nothing in Hamlet calls it yet"
)]
pub(crate) struct DecodedHttpQueueRequest {
    body: Vec<u8>,
    wait: bool,
    timeout: Option<u64>,
}

#[expect(
    dead_code,
    reason = "ported; nothing in Hamlet calls it yet"
)]
fn rearm_sleep_after_request(ctx: ActorContext) {
    let sleep_ctx = ctx.clone();
    ctx.wait_until(async move {
        sleep_ctx.wait_for_http_requests_idle().await;
        sleep_ctx.reset_sleep_timer();
    });
}

#[expect(
    dead_code,
    reason = "ported; nothing in Hamlet calls it yet"
)]
pub(crate) fn single_path_segment<'a>(
    path: &'a str,
    prefix: &str,
) -> Option<&'a str> {
    let segment = path.strip_prefix(prefix)?;
    (!segment.is_empty() && !segment.contains('/')).then_some(segment)
}

#[expect(
    dead_code,
    reason = "ported; nothing in Hamlet calls it yet"
)]
pub(crate) fn percent_decode_path_segment(
    segment: &str,
) -> Result<String> {
    let bytes = segment.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return Err(invalid_path_segment(
                    "incomplete percent escape",
                ));
            }
            let hi = hex_value(bytes[i + 1]).ok_or_else(|| {
                invalid_path_segment("invalid percent escape")
            })?;
            let lo = hex_value(bytes[i + 2]).ok_or_else(|| {
                invalid_path_segment("invalid percent escape")
            })?;
            out.push((hi << 4) | lo);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).context("path segment is not valid utf-8")
}

#[expect(
    dead_code,
    reason = "ported; nothing in Hamlet calls it yet"
)]
fn invalid_path_segment(reason: &str) -> anyhow::Error {
    ProtocolError::InvalidHttpRequest {
        field: "path segment".to_owned(),
        reason: reason.to_owned(),
    }
    .build()
}

#[expect(
    dead_code,
    reason = "ported; nothing in Hamlet calls it yet"
)]
pub(crate) fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[expect(
    dead_code,
    reason = "ported; nothing in Hamlet calls it yet"
)]
pub(crate) fn http_conn_params(
    headers: &http::HeaderMap,
) -> Result<Vec<u8>> {
    let Some(raw) = headers
        .get("x-hamlet-actor-conn-params")
        .and_then(|value| value.to_str().ok())
    else {
        return encode_json_as_cbor(&JsonValue::Null);
    };
    let value: JsonValue = serde_json::from_str(raw)
        .context("parse x-hamlet-actor-conn-params header")?;
    encode_json_as_cbor(&value)
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "ported; nothing in Hamlet calls it yet"
    )
)]
pub(crate) fn authorization_bearer_token(
    headers: &http::HeaderMap,
) -> Option<&str> {
    headers
        .get(http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(bearer_token_from_authorization)
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "ported; nothing in Hamlet calls it yet"
    )
)]
pub(crate) fn authorization_bearer_token_map(
    headers: &HashMap<String, String>,
) -> Option<&str> {
    headers
        .iter()
        .find(|(name, _)| {
            name.eq_ignore_ascii_case(
                http::header::AUTHORIZATION.as_str(),
            )
        })
        .and_then(|(_, value)| bearer_token_from_authorization(value))
}

#[expect(
    dead_code,
    reason = "ported; nothing in Hamlet calls it yet"
)]
pub(crate) fn build_http_request(
    request: HttpRequest,
) -> Result<Request> {
    let body = request.body.unwrap_or_default();
    let request_path = normalize_actor_request_path(&request.path);
    Request::from_parts_with_stream(
        &request.method,
        &request_path,
        request.headers,
        body,
        request.body_stream,
    )
    .with_context(|| {
        format!("build actor request for `{}`", request.path)
    })
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "ported; nothing in Hamlet calls it yet"
    )
)]
pub(crate) fn normalize_actor_request_path(path: &str) -> String {
    let Some(stripped) = path.strip_prefix("/request") else {
        return path.to_owned();
    };

    if stripped.is_empty() {
        return "/".to_owned();
    }

    match stripped.as_bytes().first() {
        Some(b'/') | Some(b'?') => stripped.to_owned(),
        _ => path.to_owned(),
    }
}

#[cfg(test)]
pub(crate) fn is_actor_request_path(path: &str) -> bool {
    let Some(stripped) = path.strip_prefix("/request") else {
        return false;
    };

    stripped.is_empty()
        || stripped
            .as_bytes()
            .first()
            .is_some_and(|byte| matches!(byte, b'/' | b'?'))
}

#[expect(
    dead_code,
    reason = "ported; nothing in Hamlet calls it yet"
)]
pub(crate) fn response_body_forbidden(
    request_method: &http::Method,
    status: u16,
) -> bool {
    *request_method == http::Method::HEAD
        || matches!(status, 100..=199 | 204 | 304)
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "ported; nothing in Hamlet calls it yet"
    )
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HttpResponseEncoding {
    Json,
    Cbor,
    Bare,
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "ported; nothing in Hamlet calls it yet"
    )
)]
pub(crate) fn request_encoding(
    headers: &http::HeaderMap,
) -> HttpResponseEncoding {
    headers
        .get("x-hamlet-actor-encoding")
        .and_then(|value| value.to_str().ok())
        .map(|value| match value {
            "cbor" => HttpResponseEncoding::Cbor,
            "bare" => HttpResponseEncoding::Bare,
            _ => HttpResponseEncoding::Json,
        })
        .unwrap_or(HttpResponseEncoding::Json)
}

#[expect(
    dead_code,
    reason = "ported; nothing in Hamlet calls it yet"
)]
pub(crate) fn content_type_for_encoding(
    encoding: HttpResponseEncoding,
) -> &'static str {
    match encoding {
        HttpResponseEncoding::Json => "application/json",
        HttpResponseEncoding::Cbor | HttpResponseEncoding::Bare => {
            "application/octet-stream"
        }
    }
}

/// Bounded serde metric `format` label for the request/response
/// encoding.
#[expect(
    dead_code,
    reason = "ported; nothing in Hamlet calls it yet"
)]
fn encoding_format_label(
    encoding: HttpResponseEncoding,
) -> &'static str {
    match encoding {
        HttpResponseEncoding::Json => "json",
        HttpResponseEncoding::Cbor => "cbor",
        HttpResponseEncoding::Bare => "bare",
    }
}

#[expect(
    dead_code,
    reason = "ported; nothing in Hamlet calls it yet"
)]
pub(crate) fn serialize_http_response_error(
    encoding: HttpResponseEncoding,
    group: &str,
    code: &str,
    message: &str,
    metadata: Option<&JsonValue>,
    actor: Option<&ActorSpecifier>,
) -> Result<Vec<u8>> {
    let mut json_body = json!({
        "group": group,
        "code": code,
        "message": message,
    });
    if let Some(metadata) = metadata {
        json_body["metadata"] = metadata.clone();
    }
    if let Some(actor) = actor {
        json_body["actor"] = serde_json::to_value(actor)?;
    }

    match encoding {
        HttpResponseEncoding::Json => {
            Ok(serde_json::to_vec(&json_body)?)
        }
        HttpResponseEncoding::Cbor => {
            let mut out = Vec::new();
            ciborium::into_writer(&json_body, &mut out)?;
            Ok(out)
        }
        HttpResponseEncoding::Bare => {
            let metadata = metadata
                .map(|value| {
                    let mut out = Vec::new();
                    ciborium::into_writer(value, &mut out)?;
                    Ok::<Vec<u8>, anyhow::Error>(out)
                })
                .transpose()?;
            client_protocol::versioned::HttpResponseError::serialize_with_embedded_version(
				&client_protocol::HttpResponseError {
					group: group.to_owned().into(),
					code: code.to_owned().into(),
					message: message.to_owned().into(),
					metadata: metadata.map(Into::into),
					actor: actor.map(|actor| client_protocol::ActorSpecifier {
						actor_id: actor.actor_id.clone().into(),
						generation: actor.generation,
						key: actor.key.clone().map(Into::into),
					}),
				},
				client_protocol::PROTOCOL_VERSION,
			)
        }
    }
}

#[expect(
    dead_code,
    reason = "ported; nothing in Hamlet calls it yet"
)]
pub(crate) fn decode_http_action_args(
    encoding: HttpResponseEncoding,
    body: &[u8],
) -> Result<Vec<u8>> {
    serde_metrics::measure_deserialize(
        encoding_format_label(encoding),
        "http_action_request",
        body.len(),
        || match encoding {
            HttpResponseEncoding::Json => {
                let request: HttpActionRequestJson =
                    serde_json::from_slice(body)
                        .context("decode json HTTP action request")?;
                let args = normalize_json_args(request.args);
                encode_json_as_cbor(&args)
            }
            HttpResponseEncoding::Cbor => {
                let request: HttpActionRequestJson =
                    ciborium::from_reader(Cursor::new(body))
                        .context("decode cbor HTTP action request")?;
                let args = normalize_json_args(request.args);
                encode_json_as_cbor(&args)
            }
            HttpResponseEncoding::Bare => {
                let request =
					<client_protocol::versioned::HttpActionRequest as Versioned>::deserialize_with_embedded_version(body)
						.context("decode bare HTTP action request")?;
                Ok(request.args.to_vec())
            }
        },
    )
}

#[expect(
    dead_code,
    reason = "ported; nothing in Hamlet calls it yet"
)]
fn normalize_json_args(args: JsonValue) -> Vec<JsonValue> {
    match args {
        JsonValue::Array(args) => args,
        JsonValue::Null => Vec::new(),
        value => vec![value],
    }
}

#[expect(
    dead_code,
    reason = "ported; nothing in Hamlet calls it yet"
)]
pub(crate) fn decode_http_queue_request(
    encoding: HttpResponseEncoding,
    body: &[u8],
) -> Result<DecodedHttpQueueRequest> {
    match encoding {
        HttpResponseEncoding::Json => {
            let request: HttpQueueSendRequestJson =
                serde_json::from_slice(body)
                    .context("decode json HTTP queue request")?;
            Ok(DecodedHttpQueueRequest {
                body: encode_json_as_cbor(&request.body)?,
                wait: request.wait.unwrap_or(false),
                timeout: request.timeout,
            })
        }
        HttpResponseEncoding::Cbor => {
            let request: HttpQueueSendRequestJson =
                ciborium::from_reader(Cursor::new(body))
                    .context("decode cbor HTTP queue request")?;
            Ok(DecodedHttpQueueRequest {
                body: encode_json_as_cbor(&request.body)?,
                wait: request.wait.unwrap_or(false),
                timeout: request.timeout,
            })
        }
        HttpResponseEncoding::Bare => {
            let request =
				<client_protocol::versioned::HttpQueueSendRequest as Versioned>::deserialize_with_embedded_version(body)
					.context("decode bare HTTP queue request")?;
            Ok(DecodedHttpQueueRequest {
                body: request.body.to_vec(),
                wait: request.wait.unwrap_or(false),
                timeout: request.timeout,
            })
        }
    }
}

#[expect(
    dead_code,
    reason = "ported; nothing in Hamlet calls it yet"
)]
pub(crate) fn framework_anyhow_status(
    error: &anyhow::Error,
) -> StatusCode {
    let error = ActorError::extract(error);
    framework_error_status(error.group(), error.code())
}

#[expect(
    dead_code,
    reason = "ported; nothing in Hamlet calls it yet"
)]
pub(crate) fn framework_error_status(
    group: &str,
    code: &str,
) -> StatusCode {
    if let Some(status) =
        crate::error::public_error_status_code(group, code)
    {
        return StatusCode::from_u16(status)
            .expect("core public error status must be valid");
    }
    // Preserve these HTTP-only statuses without promoting private
    // message/metadata.
    match (group, code) {
        ("actor", "invalid_request") | ("queue", _) => {
            StatusCode::BAD_REQUEST
        }
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "ported; nothing in Hamlet calls it yet"
    )
)]
fn bearer_token_from_authorization(value: &str) -> Option<&str> {
    let value = value.trim_start();
    let scheme = value.get(..6)?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }

    let rest = value.get(6..)?;
    if !rest.chars().next().is_some_and(char::is_whitespace) {
        return None;
    }

    let token = rest.trim_start();
    if token.is_empty() { None } else { Some(token) }
}

#[expect(
    dead_code,
    reason = "ported; nothing in Hamlet calls it yet"
)]
fn encode_json_as_cbor(value: &impl Serialize) -> Result<Vec<u8>> {
    let mut encoded = Vec::new();
    ciborium::into_writer(value, &mut encoded)
        .context("encode inspector payload as cbor")?;
    Ok(encoded)
}
