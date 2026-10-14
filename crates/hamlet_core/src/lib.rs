//! The runtime: the agents' loops, their sessions, what waits for a
//! chief and the hamlets as entities, as Bevy plugins with no
//! rendering (docs/core.md).

pub mod agent;
pub mod agents;
pub mod awake;
pub mod core;
pub mod oversight;
pub mod session;
pub mod terminal;
#[cfg(test)]
mod testing;
pub mod tools;
