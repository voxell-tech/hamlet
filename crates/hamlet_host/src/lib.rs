//! Runs Hamlet's actors in this process, each in one plain SQLite
//! file under the realm folder, on the host trait of
//! `hamlet_actor_core`.
#![expect(
    dead_code,
    unused_imports,
    reason = "U5's scaffold: the link's side loops in child.rs and \
              link::serve use these once U5 is integrated"
)]

mod address;
pub mod child;
mod directory;
mod event;
pub mod fault;
pub mod link;
mod local;
mod plain_sqlite;
pub mod route;

pub use address::{Address, Caller, InvalidName};
pub use event::HostEvent;
pub use local::{Host, HostOptions, HostRef, Values};
