//! What a manifest asks for.

use bevy::ecs::component::Component;

use super::name::{Capability, Refusal};
use crate::plugin::plain;

/// The hosts, agents, commands or plugins a capability is limited to.
/// Empty is no limit.
#[derive(Clone, PartialEq, Eq, Default, Debug)]
pub struct Scope(pub Vec<String>);

/// A capability a manifest asks for, and why.
#[derive(Clone, PartialEq, Debug)]
pub struct Asked {
    pub capability: Capability,
    pub reason: String,
    pub scope: Scope,
}

impl Asked {
    /// The ask of a manifest for `capability`, which must be one that
    /// can be granted and come with a reason.
    pub fn parse(
        capability: &str,
        reason: &str,
        scope: &[String],
    ) -> Result<Self, Refusal> {
        let named = Capability::parse(capability)?;
        let reason = plain(reason);
        if reason.is_empty() {
            return Err(Refusal::NoReason(capability.into()));
        }
        Ok(Self {
            capability: named,
            reason,
            scope: Scope(scope.to_vec()),
        })
    }
}

/// On the entity of an installed plugin: the most it may ask for.
#[derive(Component, Clone, PartialEq, Default, Debug)]
#[component(immutable)]
pub struct Asks(pub Vec<Asked>);

impl Asks {
    pub fn names(&self, capability: Capability) -> bool {
        self.0.iter().any(|ask| ask.capability == capability)
    }

    pub fn capabilities(&self) -> Vec<Capability> {
        self.0.iter().map(|ask| ask.capability).collect()
    }

    pub fn of(&self, capability: Capability) -> Option<&Asked> {
        self.0.iter().find(|ask| ask.capability == capability)
    }
}
