//! Runs a plugin headlessly against made-up state. A [`Harness`]
//! installs a plugin folder into a scratch home, clicks and types in
//! its panels, reads their trees as text and draws them to pictures.
//! A [`script::Script`] does the same from a TOML file.
//!
//! The parts that run a plugin are behind the `wasm` feature, as in
//! the binary.

pub mod outline;
pub mod script;

#[cfg(feature = "wasm")]
pub mod harness;
#[cfg(feature = "wasm")]
pub mod render;
#[cfg(feature = "wasm")]
pub mod run;

#[cfg(feature = "wasm")]
pub use self::harness::Harness;

#[cfg(all(test, feature = "wasm"))]
mod tests;
