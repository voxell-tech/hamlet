//! Hamlet's protocols, defined by these serde types:
//!
//! - [`client`]: the window or CLI and the server.
//! - [`runtime`]: the server and a harness runtime, such as pi or a
//!   harness written in Rust.
//!
//! [`screen`] keeps a terminal's screen as both ends of the client
//! protocol draw it.

pub mod client;
pub mod runtime;
pub mod screen;
mod version;

pub use version::{BadVersion, Version};

/// The address of the Hand.
pub const HAND: &str = "hand";
/// The agent of an address that names a hamlet's mayor, whoever holds
/// the office.
pub const MAYOR: &str = "mayor";

/// How hard a model thinks before it answers, where its provider
/// lets it be asked.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum Effort {
    Off,
    Minimal,
    Low,
    Medium,
    High,
    XHigh,
    Max,
}
