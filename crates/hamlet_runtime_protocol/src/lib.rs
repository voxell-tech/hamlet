//! The runtime protocol: the messages the server and a harness
//! runtime send each other, one JSON object per line.
//!
//! A harness written in Rust uses these types directly. The pi
//! harness validates messages against
//! `harnesses/pi/src/protocol-schema.ts`, the JSON Schema of these
//! types that `cargo test -p hamlet_runtime_protocol` writes.

#![allow(clippy::large_enum_variant)]

mod conversations;
mod handshake;
mod items;
mod models;
mod prompt;
mod providers;
mod tools;

pub use conversations::*;
pub use handshake::*;
pub use items::*;
pub use models::*;
pub use prompt::*;
pub use providers::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
pub use tools::*;

/// The runtime protocol version this server speaks.
pub const VERSION: u64 = 1;

/// A message the server sends a runtime.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum ServerMessage {
    #[serde(rename = "provider_request")]
    ProviderRequest(ProviderRequest),
    #[serde(rename = "provider_control")]
    ProviderControl(ProviderControl),
    #[serde(rename = "credential_result")]
    CredentialResult(CredentialResult),
    #[serde(rename = "welcome")]
    Welcome(Welcome),
    #[serde(rename = "tools")]
    Tools(Tools),
    #[serde(rename = "refused")]
    Refused(Refused),
    #[serde(rename = "create")]
    Create(Create),
    #[serde(rename = "submit")]
    Submit(Submit),
    #[serde(rename = "write")]
    Write(Write),
    #[serde(rename = "configure")]
    Configure(Configure),
    #[serde(rename = "fork")]
    Fork(Fork),
    #[serde(rename = "cancel")]
    Cancel(Cancel),
    #[serde(rename = "history")]
    History(History),
    #[serde(rename = "close")]
    Close(Close),
    #[serde(rename = "status")]
    Status(Status),
    #[serde(rename = "answered")]
    Answered(Answered),
    #[serde(rename = "allowed")]
    Allowed(Allowed),
    #[serde(rename = "waiting")]
    Waiting(Waiting),
    #[serde(rename = "refusal")]
    Refusal(Refusal),
    #[serde(rename = "result")]
    Result(Result),
    #[serde(rename = "allowance")]
    Allowance(Allowance),
    #[serde(rename = "stopped")]
    Stopped(Stopped),
    #[serde(rename = "charged")]
    Charged(Charged),
    #[serde(rename = "scrubbed")]
    Scrubbed(Scrubbed),
    #[serde(rename = "sections")]
    Sections(Sections),
    #[serde(rename = "verdict")]
    Verdict(Verdict),
    #[serde(rename = "error")]
    Error(ProtocolError),
}

/// A message a runtime sends the server.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum RuntimeMessage {
    #[serde(rename = "provider_result")]
    ProviderResult(ProviderResult),
    #[serde(rename = "provider_event")]
    ProviderEvent(ProviderEvent),
    #[serde(rename = "credential_request")]
    CredentialRequest(CredentialRequest),
    #[serde(rename = "hello")]
    Hello(Hello),
    #[serde(rename = "created")]
    Created(Created),
    #[serde(rename = "admitted")]
    Admitted(Admitted),
    #[serde(rename = "configured")]
    Configured(Configured),
    #[serde(rename = "cancelled")]
    Cancelled(Cancelled),
    #[serde(rename = "page")]
    Page(Page),
    #[serde(rename = "statuses")]
    Statuses(Statuses),
    #[serde(rename = "allow")]
    Allow(Allow),
    #[serde(rename = "operation")]
    Operation(Operation),
    #[serde(rename = "attempt")]
    Attempt(Attempt),
    #[serde(rename = "usage")]
    Usage(UsageReport),
    #[serde(rename = "scrub")]
    Scrub(Scrub),
    #[serde(rename = "prompt")]
    Prompt(Prompt),
    #[serde(rename = "kept")]
    Kept(Kept),
    #[serde(rename = "live")]
    Live(Live),
    #[serde(rename = "settled")]
    Settled(Settled),
    #[serde(rename = "yield")]
    Yield(Yield),
    #[serde(rename = "error")]
    Error(ProtocolError),
}

/// A message that could not be handled, in either direction; `re`
/// names it when its id was readable.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct ProtocolError {
    pub code: ErrorCode,
    #[schemars(length(min = 1))]
    pub id: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::present")]
    #[schemars(with = "String")]
    pub re: Option<String>,
    #[schemars(extend("const" = VERSION))]
    pub v: u64,
}

/// The reason a message could not be handled.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub enum ErrorCode {
    #[serde(rename = "malformed")]
    Malformed,
    #[serde(rename = "unknown_conversation")]
    UnknownConversation,
    #[serde(rename = "unsupported")]
    Unsupported,
    #[serde(rename = "failed")]
    Failed,
}

/// A field that is present, so `null` is refused rather than read as
/// absent.
pub(crate) fn present<'de, D, T>(
    deserializer: D,
) -> std::result::Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}
