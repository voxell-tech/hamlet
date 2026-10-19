//! The realm: the hamlets, the agents and where each stands, what is
//! selected and the hour. Reading it needs the capability
//! `read.world`, and doing what the chief does with a click needs
//! `world.act`. An agent is known by its name and a hamlet by its id.

pub use crate::bindings::exports::hamlet::plugin::plugin::WorldTarget;
use crate::bindings::exports::hamlet::plugin::plugin::{
    Intent, WorldOrder,
};
pub use crate::bindings::hamlet::plugin::host::{
    Place, WorldAgent, WorldHamlet, WorldThing, WorldView,
};

/// The realm as it is now: a copy, at most as many hamlets and agents
/// as the host caps. An error without `read.world`.
pub fn view() -> Result<WorldView, String> {
    crate::bindings::hamlet::plugin::host::survey()
}

/// The agent named `name`.
pub fn agent(name: &str) -> WorldTarget {
    WorldTarget::Agent(name.into())
}

/// The hamlet of the id `id`.
pub fn hamlet(id: &str) -> WorldTarget {
    WorldTarget::Hamlet(id.into())
}

/// The intent that brings `target` to the middle of the view and
/// comes in close, as the chief's Focus does. It needs `world.act`,
/// which the chief gives at its first use: the first one is dropped
/// and the answer reaches a later `step` as `Incoming::Granted` or
/// `Incoming::Denied`. To be returned from `step`.
pub fn focus(target: WorldTarget) -> Intent {
    Intent::WorldAct(WorldOrder::Focus(target))
}

/// As [`focus`], and the camera stays on `target` as the chief's
/// Follow does.
pub fn follow(target: WorldTarget) -> Intent {
    Intent::WorldAct(WorldOrder::Follow(target))
}

/// The intent that selects `target`, as a click on it does. Gated as
/// [`focus`] is.
pub fn select(target: WorldTarget) -> Intent {
    Intent::WorldAct(WorldOrder::Select(target))
}
