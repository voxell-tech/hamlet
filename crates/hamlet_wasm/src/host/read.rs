//! The reads a plugin may make, each needing a grant. What a read
//! returns is a copy.

use bevy::math::Vec3;
use hamlet_sdk::capability::{Capability, Verdict};

use super::State;
use crate::interface::hamlet::plugin::host::{
    Agent, AgentPapers, Place, WorldAgent, WorldHamlet, WorldThing,
    WorldView,
};

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

    pub(super) fn read_world(&mut self) -> Result<WorldView, String> {
        self.require(Capability::ReadWorld)?;
        let place = |at: Option<Vec3>| {
            at.map(|at| Place {
                x: at.x,
                y: at.y,
                z: at.z,
            })
        };
        let survey = &self.survey;
        Ok(WorldView {
            hamlets: survey
                .hamlets
                .iter()
                .take(self.world_hamlets)
                .map(|hamlet| WorldHamlet {
                    id: hamlet.id.clone(),
                    name: hamlet.name.clone(),
                    at: place(hamlet.at),
                })
                .collect(),
            agents: survey
                .agents
                .iter()
                .take(self.world_agents)
                .map(|agent| WorldAgent {
                    name: agent.name.clone(),
                    hamlet: agent.hamlet.clone(),
                    at: place(agent.at),
                    busy: agent.busy,
                })
                .collect(),
            selected: survey.selected.as_ref().map(|spotted| {
                WorldThing {
                    kind: spotted.kind.clone(),
                    name: spotted.name.clone(),
                }
            }),
            hour: survey.hour,
        })
    }

    /// Whether the plugin holds `capability`. A refusal is logged
    /// with the plugin's id, and where the chief is yet to decide
    /// it is noted as wanted.
    pub(crate) fn require(
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
        let why = format!("{} not granted", capability.name());
        self.refused(capability.name(), &why);
        Err(why)
    }
}
