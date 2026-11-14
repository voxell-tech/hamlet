//! Runs Hamlet's actors in this process, each in one plain SQLite
//! file under the realm folder, on the host trait of
//! `hamlet_actor_core`.

mod cause;
pub mod child;
mod directory;
mod event;
pub mod fault;
pub mod link;
mod local;
mod plain_sqlite;

pub use cause::Cause;
pub use directory::Address;
pub use event::HostEvent;
pub use local::{Caller, Host, HostOptions, HostRef, Values};
