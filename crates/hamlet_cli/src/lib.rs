//! Hamlet's commands and process lifetimes, shared by `hamlet`, the
//! command, and `hamlet_launch`, which a packaged app opens.

#![expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "a terminal program: printing is its job"
)]

pub mod chat;
pub mod client;
pub mod launch;
pub mod process;
pub mod terminal;
