//! The Hamlet server: the realm, its society, and its agents'
//! conversations and work.
#![expect(
    dead_code,
    reason = "the rebuild wires its parts up in steps"
)]

mod change;
mod claim;
mod clients;
mod clock;
mod commands;
mod core;
mod credentials;
mod diagnostic;
mod fault;
mod jobs;
mod knowledge;
mod layout;
mod realm;
pub mod serve;
mod settings;
mod society;
mod spend;
mod talk;
#[cfg(test)]
mod testing;
mod text;
mod work;
