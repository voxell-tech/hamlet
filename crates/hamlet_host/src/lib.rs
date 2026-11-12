//! Runs Hamlet's actors in this process, each in one plain SQLite
//! file under the realm folder, on the host trait of
//! `hamlet_actor_core`.

pub mod fault;
mod local;
mod plain_sqlite;

pub use local::{Host, now_ms};
