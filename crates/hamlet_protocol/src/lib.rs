//! The client protocol: messages exchanged by the window or CLI and
//! the server. These serde types are the protocol; `docs/protocol.md`
//! describes it.

pub mod discovery;
pub mod transcript;

/// The first hamlet's id.
pub const HOME: &str = "home";
/// The agent of an address that names a hamlet's mayor, whoever holds
/// the office.
pub const MAYOR: &str = "mayor";

mod messages;

pub use messages::*;

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
