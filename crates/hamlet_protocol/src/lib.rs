//! The client protocol: messages exchanged by the window or CLI and
//! the server. These serde types are the protocol; `docs/protocol.md`
//! describes it.

#![allow(clippy::large_enum_variant)]

pub mod discovery;
pub mod transcript;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The first hamlet's id.
pub const HOME: &str = "home";
/// The agent of an address that names a hamlet's mayor, whoever holds
/// the office.
pub const MAYOR: &str = "mayor";

mod chat;
mod documents;
mod gifts;
mod limits;
mod settings;
mod state;
mod waits;

pub use chat::*;
pub use documents::*;
pub use gifts::*;
pub use limits::*;
pub use settings::*;
pub use state::*;
pub use waits::*;

impl ServerMessage {
    /// The request this message answers, if any.
    pub fn to(&self) -> Option<&str> {
        match self {
            Self::Reply(reply) => Some(&reply.to),
            Self::Fault(fault) => fault.to.as_deref(),
            Self::Transcript(transcript) => transcript.to.as_deref(),
            Self::Login(login) => login.to.as_deref(),
            Self::Read(
                ReadResult::Prompt { to, .. }
                | ReadResult::History { to, .. }
                | ReadResult::Models { to, .. }
                | ReadResult::Papers { to, .. }
                | ReadResult::Charter { to, .. },
            ) => Some(to),
            Self::State(_) | Self::Change(_) | Self::Turn(_) => None,
        }
    }
}

/// The value of every `v` field.
pub const V: u64 = 0;

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum ClientMessage {
    #[serde(rename = "hello")]
    Hello(Hello),
    #[serde(rename = "say")]
    Say(Say),
    #[serde(rename = "give")]
    Give(Give),
    #[serde(rename = "give-part")]
    GivePart(GivePart),
    #[serde(rename = "answer")]
    Answer(Answer),
    #[serde(rename = "stop")]
    Stop(Stop),
    #[serde(rename = "rewind")]
    Rewind(Rewind),
    #[serde(rename = "new")]
    New(New),
    #[serde(rename = "resume")]
    Resume(Resume),
    #[serde(rename = "retry")]
    Retry(Retry),
    #[serde(rename = "edit")]
    Edit(Edit),
    #[serde(rename = "open")]
    Open(Open),
    #[serde(rename = "close")]
    Close(Close),
    #[serde(rename = "login")]
    Login(Login),
    #[serde(rename = "check")]
    Check(Check),
    #[serde(rename = "read")]
    Read(Read),
    #[serde(rename = "move-houses")]
    MoveHouses(MoveHouses),
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum ServerMessage {
    #[serde(rename = "state")]
    State(State),
    #[serde(rename = "change")]
    Change(Changes),
    #[serde(rename = "turn")]
    Turn(Turn),
    #[serde(rename = "transcript")]
    Transcript(Transcript),
    #[serde(rename = "reply")]
    Reply(Reply),
    #[serde(rename = "login")]
    Login(LoginProgress),
    #[serde(rename = "read")]
    Read(ReadResult),
    #[serde(rename = "fault")]
    Fault(Fault),
}

pub type RequestId = String;

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Hello {
    pub token: String,
    #[schemars(extend("const" = V))]
    pub v: u64,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    pub kind: ReplyKind,
    pub text: String,
    #[schemars(extend("format" = "uuid"))]
    pub to: RequestId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::present")]
    #[schemars(with = "String")]
    pub upload: Option<String>,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub enum ReplyKind {
    #[serde(rename = "notice")]
    Notice,
    #[serde(rename = "error")]
    Error,
    #[serde(rename = "rewound")]
    Rewound,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Fault {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::present")]
    #[schemars(with = "RequestId", extend("format" = "uuid"))]
    pub to: Option<RequestId>,
}

pub(crate) fn present<'de, D, T>(
    deserializer: D,
) -> std::result::Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}
