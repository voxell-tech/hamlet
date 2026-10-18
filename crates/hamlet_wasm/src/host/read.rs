//! The reads a plugin may make, each needing a grant. What a read
//! returns is a copy.

use bevy::log::warn;
use hamlet_sdk::grant::READ_AGENTS;

use super::State;
use crate::interface::hamlet::plugin::host::Agent;

impl State {
    pub(super) fn read_agents(&self) -> Result<Vec<Agent>, String> {
        self.require(READ_AGENTS)?;
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
    fn require(&self, capability: &str) -> Result<(), String> {
        if self.granted.iter().any(|held| held == capability) {
            return Ok(());
        }
        warn!("plugin {}: {capability} not granted", self.plugin);
        Err(format!("{capability} not granted"))
    }
}
