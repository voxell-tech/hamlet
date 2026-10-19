//! The runtime: the agents' loops, their sessions, what waits for a
//! chief and the hamlets as entities, with no rendering
//! (docs/core.md). A module's `plugin` adds its own systems, and an
//! app adds the modules it runs.

pub mod agent;
pub mod agents;
pub mod catalogue;
pub mod core;
pub mod oversight;
pub mod session;
pub mod terminal;
#[cfg(test)]
mod testing;
pub mod tools;

use bevy_app::App;

/// The modules every app that runs the agents adds, in their order:
/// the agents layer and this crate's tests. Insert the resources
/// `core` reads before it.
pub fn runtime(app: &mut App) {
    app.add_plugins((
        core::plugin,
        agents::plugin,
        catalogue::plugin,
        session::plugin,
    ));
}
