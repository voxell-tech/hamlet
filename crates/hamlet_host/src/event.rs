//! What a host reports about its actors, on [`Host::events`].
//!
//! [`Host::events`]: crate::Host::events

use crate::Address;

/// Something that happened to an actor of the host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostEvent {
    /// The host gave up on `letter`, which `holder` kept, after
    /// `tries` tries. `error` is the last try's outcome as
    /// `group.code`, `None` when none came back.
    DeadLetter {
        holder: Address,
        letter: Letter,
        tries: u32,
        error: Option<String>,
    },
}

/// The work a dead letter was.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Letter {
    /// A one-shot timer, set aside: it stays in its actor's file,
    /// and [`Host::rearm_timer`] puts it back.
    ///
    /// [`Host::rearm_timer`]: crate::Host::rearm_timer
    Timer {
        event_id: String,
        action: String,
        /// The due time it had, in ms since the Unix epoch.
        due_at: i64,
    },
}
