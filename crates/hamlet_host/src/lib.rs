//! Runs Hamlet's actors in this process, each in one plain SQLite
//! file under the realm folder, on the host trait of
//! `hamlet_actor_core`.

mod directory;
mod event;
pub mod fault;
mod local;
mod plain_sqlite;

pub use event::HostEvent;
/// The host's calls take an [`Address`] and stamp a [`Caller`].
pub use hamlet_address::{Address, Caller, InvalidName};
pub use local::{Host, HostOptions, HostRef, Values};
