//! What a host reports about its actors, on [`Host::events`].
//!
//! [`Host::events`]: crate::Host::events

use crate::Address;

/// Something that happened to an actor of the host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostEvent {
    /// The host gave up on `letter`, which `holder` kept, after
    /// `tries` tries. `error` is the last try's outcome, `None` when
    /// none came back: a timer's as `group.code`, a send's as the
    /// text of its handler's error.
    ///
    /// The event is a notice and may come twice for one letter, as
    /// when a kill lands before the host has confirmed it; the
    /// letter's fields tell it is the same one. The holder's file
    /// keeps the record, once.
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
    /// A send, set aside: it stays in the holder's
    /// `_hamlet_dead_letters`. Its receiver holds it when its handler
    /// failed, and its sender when it had no address. `peer`,
    /// `peer_file` and `seq` name it.
    Send {
        /// The pair's other actor: the sender, when the receiver
        /// holds the letter; the receiver, when the sender does.
        peer: Address,
        /// Which life of the peer: the sender file's id. `None` for
        /// a receiver with no address, which has no file.
        peer_file: Option<String>,
        /// The send's number within its pair.
        seq: u64,
        name: String,
        reason: Reason,
    },
}

/// Why a send was set aside.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    /// Its handler's runs ended without success too many times.
    Attempts,
    /// The host holds no actor at its receiver's address: the kind is
    /// not one of the host's, or the key has no file.
    NoAddress,
}
