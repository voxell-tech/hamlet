//! The kinds a demo host runs: the ones its `--kind` options name,
//! or a default.

use std::collections::HashMap;

use hamlet_actor_core::ActorFactory;
use hamlet_actors::workspace;
use hamlet_messages::address::{self, Party};

use crate::{agent, counter, realm};

/// The kinds with a factory of their own. Any other is a counter.
const NAMED: [(&str, fn() -> ActorFactory); 3] = [
    (address::Workspace::KIND, workspace::factory),
    (address::Realm::KIND, realm::factory),
    (address::Agent::KIND, agent::factory),
];

/// What a host runs when no `--kind` says.
pub const LOCAL: &[&str] = &[
    "counter",
    address::Workspace::KIND,
    address::Realm::KIND,
    address::Agent::KIND,
];

/// What the Rust host of a linked demo runs when no `--kind` says.
/// Its Node host runs [`link::NODE_KINDS`](crate::link::NODE_KINDS).
pub const LINKED: &[&str] = &[address::Realm::KIND];

/// The factory of each of `kinds`.
pub fn factories(
    kinds: &[String],
    sleep: counter::Sleep,
) -> HashMap<String, ActorFactory> {
    let counters = counters(kinds);
    let mut factories = counter::factories(
        &counters.iter().map(String::as_str).collect::<Vec<_>>(),
        sleep,
    );
    for (kind, factory) in NAMED {
        if kinds.iter().any(|named| named == kind) {
            factories.insert(kind.to_owned(), factory());
        }
    }
    factories
}

/// The counter kinds among `kinds`, in their order.
pub fn counters(kinds: &[String]) -> Vec<String> {
    kinds
        .iter()
        .filter(|kind| NAMED.iter().all(|(named, _)| named != kind))
        .cloned()
        .collect()
}
