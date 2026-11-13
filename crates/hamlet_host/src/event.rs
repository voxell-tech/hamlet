//! What a host reports about its actors, on [`Host::events`].
//!
//! [`Host::events`]: crate::Host::events

use crate::Address;

/// Something that happened to an actor of the host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostEvent {
    /// The one-shot timer `event_id` of the actor at `actor` failed
    /// its last try, the `tries`-th, and is set aside: its actor is
    /// not woken for it again. It stays in the actor's file, and
    /// [`Host::rearm_timer`] puts it back.
    ///
    /// [`Host::rearm_timer`]: crate::Host::rearm_timer
    TimerSetAside {
        actor: Address,
        event_id: String,
        action: String,
        /// The due time it had, in ms since the Unix epoch.
        due_at: i64,
        tries: u32,
        /// The last try's outcome as `group.code`, `None` when none
        /// came back.
        error: Option<String>,
    },
}
