//! Hamlet's protocols, defined by these serde types:
//!
//! - [`client`]: the window or CLI and the server.
//! - [`runtime`]: the server and a harness runtime, such as pi or a
//!   harness written in Rust.
//!
//! And [`vault`], the form both the window and the server seal a
//! Hamlet account's OpenRouter key in.

pub mod client;
pub mod runtime;
pub mod vault;

/// The first hamlet's id.
pub const HOME: &str = "home";
/// The agent of an address that names a hamlet's mayor, whoever holds
/// the office.
pub const MAYOR: &str = "mayor";
