//! Messages exchanged by the window and agent server.

pub mod discovery;
pub mod transcript;

/// The first hamlet's id.
pub const HOME: &str = "home";
/// The agent of an address that names a hamlet's mayor, whoever holds
/// the office.
pub const MAYOR: &str = "mayor";

#[rustfmt::skip]
pub mod v0;

impl v0::ServerMessage {
    /// The request this message answers, if any.
    pub fn to(&self) -> Option<&str> {
        match self {
            Self::Reply(reply) => Some(&reply.to),
            Self::Fault(fault) => fault.to.as_deref(),
            Self::Transcript(transcript) => transcript.to.as_deref(),
            Self::Login(login) => login.to.as_deref(),
            Self::Read(
                v0::ReadResult::Prompt { to, .. }
                | v0::ReadResult::History { to, .. }
                | v0::ReadResult::Models { to, .. }
                | v0::ReadResult::Papers { to, .. }
                | v0::ReadResult::Charter { to, .. },
            ) => Some(to),
            Self::State(_) | Self::Change(_) | Self::Turn(_) => None,
        }
    }
}
