//! The Hamlet server: a home claimed by one process (S2), its
//! realm database and the operations harnesses ask of it, each made
//! once.

// The workspace allows this for Bevy systems; the server has none.
#![warn(clippy::too_many_arguments)]

mod change;
mod chats;
mod claim;
mod clock;
mod connection;
mod credentials;
mod diagnostic;
mod documents;
mod fault;
mod gate;
mod gifts;
mod hand;
mod harness;
#[cfg(feature = "harness")]
pub mod harness_driver;
mod home;
mod limits;
mod market;
mod models;
mod operation;
mod oversight;
mod papers;
mod prompt;
mod protocol;
mod realm;
mod record;
mod requests;
mod runtime;
mod sandbox;
mod scrub;
pub mod serve;
mod service;
mod settings;
mod society;
mod spend;
mod starters;
mod state;
mod store;
#[cfg(test)]
mod testing;
mod tools;
mod workspace;

pub use claim::Held;
pub use home::Home;
use home::Layout;
use operation::Operation;
pub use operation::{OperationId, Outcome, Reply, RuntimeInstance};
pub use record::{
    Actor, Admit, Change, End, FoundRecord, Hamlet, Limits,
    RecordOperation, RoadBudget,
};
use settings::Overrides;
pub use store::Store;
// The harness tests read a conversation's charges.
#[cfg(feature = "harness")]
pub use {harness::BindingId, spend::usage};
