//! The reads a plugin may make, each needing a grant. What a read
//! returns is a copy.

use bevy::log::warn;
use hamlet_sdk::capability::Capability;

use super::State;
use crate::interface::hamlet::plugin::host::Agent;

impl State {
    pub(super) fn read_agents(&self) -> Result<Vec<Agent>, String> {
        self.require(Capability::ReadAgents)?;
        Ok(self
            .agents
            .iter()
            .map(|agent| Agent {
                name: agent.name.clone(),
                hamlet: agent.hamlet.clone(),
            })
            .collect())
    }

    /// Whether the plugin holds `capability`. A denial is logged with
    /// the plugin's id.
    fn require(&self, capability: Capability) -> Result<(), String> {
        if self.granted.holds(capability) {
            return Ok(());
        }
        let name = capability.name();
        warn!("plugin {}: {name} not granted", self.plugin);
        Err(format!("{name} not granted"))
    }
}
