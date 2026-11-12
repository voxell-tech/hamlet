// This file was changed for Hamlet. See NOTICE in this crate.
pub mod generated;
pub mod versioned;

// Re-export latest.
pub use generated::v6::*;

pub const PROTOCOL_VERSION: u16 = 6;
