//! The network service. A plugin asks for `net.fetch` on each host it
//! needs, and the person answers for each. A fetch goes over `http`
//! or `https` to such a host, at the addresses the engine resolved
//! for it, with no redirect followed and its size and time capped,
//! off the frame.

mod address;
mod fetch;

#[cfg(test)]
mod tests;

use bevy::app::App;

pub use crate::address::{reach, reachable};
pub use crate::fetch::{Fetch, NetWorldExt, Offline};

pub fn plugin(app: &mut App) {
    app.add_plugins(fetch::plugin);
}
