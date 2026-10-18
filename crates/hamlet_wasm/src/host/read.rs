//! The reads a plugin may make, each needing a grant. What a read
//! returns is a copy.

use bevy::log::warn;
use hamlet_sdk::capability::{Capability, Verdict};

use super::State;
use crate::interface::hamlet::plugin::host::{Agent, AgentPapers};

impl State {
    pub(super) fn read_agents(
        &mut self,
    ) -> Result<Vec<Agent>, String> {
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

    pub(super) fn read_papers(
        &mut self,
    ) -> Result<AgentPapers, String> {
        self.require(Capability::ReadPapers)?;
        Ok(AgentPapers {
            notes: self.papers.notes.clone(),
            character: self.papers.character.clone(),
            skills: self
                .papers
                .skills
                .iter()
                .map(|(name, _)| name.clone())
                .collect(),
        })
    }

    /// Whether the plugin holds `capability`. A refusal is logged
    /// with the plugin's id, and where the chief is yet to decide
    /// it is noted as wanted.
    fn require(
        &mut self,
        capability: Capability,
    ) -> Result<(), String> {
        let verdict = self.granted.verdict(capability);
        if verdict == Some(Verdict::Allowed) {
            return Ok(());
        }
        if verdict == Some(Verdict::Ask) {
            self.want(capability, "");
        }
        let name = capability.name();
        warn!("plugin {}: {name} not granted", self.plugin);
        Err(format!("{name} not granted"))
    }
}
