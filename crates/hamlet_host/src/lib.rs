//! Runs Hamlet's actors in this process, each in one plain SQLite
//! file under the realm folder, on the host trait of
//! `hamlet_actor_core`.

mod directory;
mod event;
pub mod fault;
mod local;
mod plain_sqlite;

pub use directory::Address;
pub use event::HostEvent;
pub use local::{Caller, Host, HostOptions, HostRef, Values};
