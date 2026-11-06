//! Hamlet's protocols, defined by these serde types:
//!
//! - [`client`]: the window or CLI and the server.
//! - [`runtime`]: the server and a harness runtime, such as pi or a
//!   harness written in Rust.

pub mod client;
pub mod runtime;

/// The first hamlet's id.
pub const HOME: &str = "home";
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
}
