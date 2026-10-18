//! What an installed plugin may do beyond what every plugin may.

use bevy::prelude::*;

use crate::manifest::Ask;

/// The capability to read the agents and their hamlets.
pub const READ_AGENTS: &str = "read.agents";

/// On the entity of an installed plugin: the capabilities it holds,
/// by name. A host function that needs one checks it on each call,
/// so a component put in its place is seen by the next call.
#[derive(Component, Clone, PartialEq, Debug, Default)]
#[component(immutable)]
pub struct Granted(pub Vec<String>);

impl Granted {
    pub fn holds(&self, capability: &str) -> bool {
        self.0.iter().any(|held| held == capability)
    }

    /// The capabilities of `approved` that the manifest asked for.
    /// The rest are impossible for the plugin, so they are
    /// dropped.
    pub fn within(
        id: &str,
        approved: &[String],
        asks: &[Ask],
    ) -> Self {
        let (asked, unasked) = approved
            .iter()
            .cloned()
            .partition::<Vec<_>, _>(|capability| {
                asks.iter().any(|ask| ask.capability == *capability)
            });
        for capability in unasked {
            warn!(
                "plugin {id}: {capability} is approved but the \
                 manifest does not ask for it"
            );
        }
        Self(asked)
    }
}
