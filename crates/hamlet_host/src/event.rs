//! What a host reports about its actors, on [`Host::events`].
//!
//! [`Host::events`]: crate::Host::events

use serde::{Deserialize, Serialize};

use crate::route::Operation;
use crate::{Address, Caller};

/// Something that happened to an actor of the host.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostEvent {
    /// The one-shot timer `event_id` of the actor at `actor` used up
    /// its `tries` tries with no answer from its handler, and is set
    /// aside: its actor is not woken for it again. It stays in the
    /// actor's file, and [`Host::rearm_timer`] puts it back.
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
    /// Boot could not read the file of the actor at `actor`, and
    /// booted the rest without it. `error` says why.
    UnreadableFile { actor: Address, error: String },
    /// The route table refused `from`'s `operation` on the actor at
    /// `to`.
    Refused {
        from: Caller,
        to: Address,
        operation: Operation,
    },
}
