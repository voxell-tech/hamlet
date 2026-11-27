//! Hamlet's client protocol, between the window or CLI and the
//! server, defined by the serde types in [`client`].

pub mod client;
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
