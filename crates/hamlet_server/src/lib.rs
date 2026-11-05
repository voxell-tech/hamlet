//! The Hamlet server: the realm, its society, and its agents'
//! conversations and work.
#![expect(
    dead_code,
    reason = "the rebuild serves nothing until its loop is written"
)]

mod change;
mod claim;
mod clock;
mod core;
mod credentials;
mod diagnostic;
mod fault;
mod jobs;
mod knowledge;
mod layout;
mod realm;
mod settings;
mod society;
mod talk;
#[cfg(test)]
mod testing;
mod text;
mod work;
