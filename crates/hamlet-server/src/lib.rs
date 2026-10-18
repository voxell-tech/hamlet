//! The Rust Hamlet server: a home claimed by one process (S2), its
//! realm database and the operations harnesses ask of it, each made
//! once.

mod claim;
mod credentials;
mod documents;
mod fault;
pub mod harness;
mod home;
mod limits;
mod models;
mod operation;
mod papers;
mod prompt;
pub mod protocol;
pub mod record;
pub mod runtime;
pub mod service;
mod settings;
pub mod spend;
mod state;
mod store;

pub use claim::Held;
pub use home::{Home, Layout};
pub use operation::{
    Operation, OperationId, Outcome, Reply, RuntimeInstance,
};
pub use store::Store;
