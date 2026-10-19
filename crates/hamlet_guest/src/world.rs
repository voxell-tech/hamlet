//! The realm: the hamlets, the agents and where each stands, what is
//! selected and the hour. Reading it needs the capability
//! `read.world`. An agent is known by its name and a hamlet by its
//! id.

pub use crate::bindings::hamlet::plugin::host::{
    Place, WorldAgent, WorldHamlet, WorldThing, WorldView,
};

/// The realm as it is now: a copy, at most as many hamlets and agents
/// as the host caps. An error without `read.world`.
pub fn view() -> Result<WorldView, String> {
    crate::bindings::hamlet::plugin::host::survey()
}
