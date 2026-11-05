//! The Hamlet server: the realm, its society, and its agents'
//! conversations and work.
#![expect(
    dead_code,
    reason = "the rebuild serves nothing until its loop is written"
)]

mod change;
mod clock;
mod jobs;
mod knowledge;
mod layout;
mod society;
#[cfg(test)]
mod testing;
mod text;
