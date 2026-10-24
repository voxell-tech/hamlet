//! The society the world is drawn from: towns, agents and the roads
//! between them.

mod places;
#[cfg(test)]
mod tests;

use std::hash::{DefaultHasher, Hash, Hasher};

use bevy::prelude::*;

pub use self::places::{Claim, Ground};
use super::house::{HouseSettings, Remains};
use super::land::Land;
use super::layout::plot;
use crate::keeper::KeeperState;
use crate::keeper::work::Work;

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord,
)]
pub struct AgentId(pub u32);

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord,
)]
pub struct TownId(pub u32);

/// The use of a road.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord,
)]
pub enum RoadLevel {
    /// Unused and growing over.
    Overgrown,
    Dirt,
    Paved,
    Rail,
}

#[derive(Clone, Debug, PartialEq, Hash)]
pub struct Town {
    pub id: TownId,
    /// The site it was founded on. Never changes: a town that is
    /// moved takes its site along.
    pub site: usize,
    pub mayor: AgentId,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Agent {
    pub id: AgentId,
    pub town: TownId,
    /// The plot its house stands on. Changes only when it moves
    /// town.
    pub plot: u32,
    pub state: KeeperState,
    /// Its task while it works.
    pub work: Work,
    /// Removed or merged away. Its plot is free for a newcomer.
    pub archived: bool,
    /// A newcomer has built on its plot since it left, so nothing
    /// of its house remains.
    pub cleared: bool,
    /// The agent it was merged into, while it is archived.
    pub into: Option<AgentId>,
    /// Houses merged into this one.
    pub wings: u8,
}

impl Agent {
    /// The remains of its house once it has gone out. None while it
    /// lives.
    pub fn remains(&self, set: &HouseSettings) -> Option<Remains> {
        if !self.archived {
            return None;
        }
        if self.cleared {
            return Some(Remains::Bare);
        }
        Some(match self.into {
            Some(_) => set.merged,
            None => set.removed,
        })
    }
}

/// The two agents a road joins, lower id first.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord,
)]
pub struct Pair(pub AgentId, pub AgentId);

impl Pair {
    pub fn new(a: AgentId, b: AgentId) -> Self {
        if a <= b { Self(a, b) } else { Self(b, a) }
    }

    pub fn has_end(&self, id: AgentId) -> bool {
        self.0 == id || self.1 == id
    }
}

#[derive(Clone, Debug, PartialEq, Hash)]
pub struct Road {
    pub pair: Pair,
    pub level: RoadLevel,
}

/// Everything the world shows.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct Society {
    pub towns: Vec<Town>,
    pub agents: Vec<Agent>,
    pub roads: Vec<Road>,
    /// Ground no town may stand on.
    pub claims: Vec<Claim>,
    next_agent: u32,
    next_town: u32,
}

/// The outcome of a change asked of the [`Society`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Changed,
    /// Nothing was changed: what was asked does not hold.
    Unchanged,
}

impl Society {
    /// A number for everything that decides what is built: all but
    /// what each agent is doing.
    pub(super) fn shape(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.towns.hash(&mut hasher);
        self.roads.hash(&mut hasher);
        self.agents.len().hash(&mut hasher);
        for a in &self.agents {
            (a.id, a.town, a.plot).hash(&mut hasher);
            (a.archived, a.cleared, a.into, a.wings)
                .hash(&mut hasher);
        }
        hasher.finish()
    }

    pub fn town(&self, id: TownId) -> Option<&Town> {
        self.towns.iter().find(|t| t.id == id)
    }

    /// The index of the agent `id` in `agents`, which are kept in the
    /// order they were given their ids.
    fn index(&self, id: AgentId) -> Option<usize> {
        self.agents.binary_search_by_key(&id, |a| a.id).ok()
    }

    pub fn agent(&self, id: AgentId) -> Option<&Agent> {
        self.index(id).map(|at| &self.agents[at])
    }

    fn agent_mut(&mut self, id: AgentId) -> Option<&mut Agent> {
        self.index(id).map(|at| &mut self.agents[at])
    }

    /// The living agents of a town.
    pub fn agents_of(
        &self,
        town: TownId,
    ) -> impl Iterator<Item = &Agent> {
        self.agents
            .iter()
            .filter(move |a| a.town == town && !a.archived)
    }

    pub fn road(&self, pair: Pair) -> Option<&Road> {
        self.roads.iter().find(|r| r.pair == pair)
    }

    fn spawn_agent(&mut self, town: TownId, plot: u32) -> AgentId {
        let id = AgentId(self.next_agent);
        self.next_agent += 1;
        self.agents.push(Agent {
            id,
            town,
            plot,
            state: KeeperState::Rest,
            work: Work::default(),
            archived: false,
            cleared: false,
            into: None,
            wings: 0,
        });
        id
    }

    /// The lowest plot of a town that is fit to build on and holds
    /// no house. What is left of a house that is gone holds none.
    fn free_plot(&self, land: &Land, town: &Town) -> Option<u32> {
        (0..land.set.town.plots).find(|&k| {
            plot(land, town.site, k).usable
                && !self.agents_of(town.id).any(|a| a.plot == k)
        })
    }

    /// Clears what is left of the houses that stood on a plot.
    fn clear(&mut self, town: TownId, plot: u32) {
        for a in &mut self.agents {
            if a.archived && a.town == town && a.plot == plot {
                a.cleared = true;
            }
        }
    }

    /// True when a town has a plot left for one more agent.
    pub(crate) fn has_room(&self, land: &Land, town: TownId) -> bool {
        self.town(town)
            .is_some_and(|town| self.free_plot(land, town).is_some())
    }

    /// Adds an agent with a house and a road to its mayor. None
    /// when the town has no plot left.
    pub fn add_agent(
        &mut self,
        land: &Land,
        town: TownId,
    ) -> Option<AgentId> {
        let t = self.town(town)?.clone();
        let plot = self.free_plot(land, &t)?;
        self.clear(town, plot);
        let id = self.spawn_agent(town, plot);
        self.add_road(id, t.mayor);
        Some(id)
    }

    /// Lays a dirt road, unless the two already have one.
    pub fn add_road(&mut self, a: AgentId, b: AgentId) -> Outcome {
        let pair = Pair::new(a, b);
        let live = |id| self.agent(id).is_some_and(|a| !a.archived);
        if a == b || !live(a) || !live(b) || self.road(pair).is_some()
        {
            return Outcome::Unchanged;
        }
        self.roads.push(Road {
            pair,
            level: RoadLevel::Dirt,
        });
        Outcome::Changed
    }

    pub fn set_level(&mut self, pair: Pair, level: RoadLevel) {
        if let Some(r) =
            self.roads.iter_mut().find(|r| r.pair == pair)
        {
            r.level = level;
        }
    }

    pub fn set_state(&mut self, id: AgentId, state: KeeperState) {
        if let Some(a) = self.agent_mut(id) {
            a.state = state;
        }
    }

    pub fn set_work(&mut self, id: AgentId, work: Work) {
        if let Some(a) = self.agent_mut(id) {
            a.work = work;
        }
    }

    /// Takes a town out of the world with everyone in it, its hall
    /// and the roads to it. Its site is free to be founded on again.
    pub(crate) fn remove_town(&mut self, id: TownId) -> Outcome {
        if self.town(id).is_none() {
            return Outcome::Unchanged;
        }
        let gone = self
            .agents
            .iter()
            .filter(|a| a.town == id)
            .map(|a| a.id)
            .collect::<Vec<_>>();
        self.roads.retain(|r| {
            !gone.iter().any(|agent| r.pair.has_end(*agent))
        });
        self.agents.retain(|a| a.town != id);
        self.towns.retain(|t| t.id != id);
        Outcome::Changed
    }

    /// Archives an agent: its house becomes a ruin and its roads go.
    /// A mayor cannot be removed.
    pub fn remove_agent(&mut self, id: AgentId) -> Outcome {
        if self.towns.iter().any(|t| t.mayor == id) {
            return Outcome::Unchanged;
        }
        let Some(agent) = self.agent_mut(id) else {
            return Outcome::Unchanged;
        };
        agent.archived = true;
        self.roads.retain(|r| !r.pair.has_end(id));
        Outcome::Changed
    }

    /// Brings an archived agent back, with a road to its mayor.
    /// Its house stands where it stood, or on another plot if a
    /// newcomer has built there since. Unchanged when its town has no
    /// plot left for it.
    pub fn restore(&mut self, land: &Land, id: AgentId) -> Outcome {
        let Some(agent) = self.agent(id).cloned() else {
            return Outcome::Unchanged;
        };
        let Some(town) = self.town(agent.town).cloned() else {
            return Outcome::Unchanged;
        };
        let taken =
            self.agents_of(town.id).any(|a| a.plot == agent.plot);
        let plot = if taken || agent.cleared {
            match self.free_plot(land, &town) {
                Some(plot) => plot,
                None => return Outcome::Unchanged,
            }
        } else {
            agent.plot
        };
        self.clear(town.id, plot);
        if let Some(agent) = self.agent_mut(id) {
            agent.archived = false;
            agent.cleared = false;
            agent.into = None;
            agent.plot = plot;
        }
        self.add_road(id, town.mayor);
        Outcome::Changed
    }

    /// Undoes a merge: `gone` comes back and `kept` loses the wing.
    /// The roads stay as they are.
    pub fn split(
        &mut self,
        land: &Land,
        kept: AgentId,
        gone: AgentId,
    ) -> Outcome {
        if self.restore(land, gone) == Outcome::Unchanged {
            return Outcome::Unchanged;
        }
        if let Some(a) = self.agent_mut(kept) {
            a.wings = a.wings.saturating_sub(1);
        }
        Outcome::Changed
    }

    /// Merges `gone` into `kept`: `kept` gains a wing and the roads
    /// of both, and `gone`'s house is archived.
    pub fn merge(&mut self, kept: AgentId, gone: AgentId) -> Outcome {
        let is_mayor = |id| self.towns.iter().any(|t| t.mayor == id);
        if kept == gone || is_mayor(gone) {
            return Outcome::Unchanged;
        }
        let moved = self
            .roads
            .iter()
            .filter(|r| r.pair.has_end(gone) && !r.pair.has_end(kept))
            .map(|r| {
                let other = if r.pair.0 == gone {
                    r.pair.1
                } else {
                    r.pair.0
                };
                (Pair::new(kept, other), r.level)
            })
            .collect::<Vec<_>>();
        if self.remove_agent(gone) == Outcome::Unchanged {
            return Outcome::Unchanged;
        }
        for (pair, level) in moved {
            match self.roads.iter_mut().find(|r| r.pair == pair) {
                Some(r) => r.level = r.level.max(level),
                None => self.roads.push(Road { pair, level }),
            }
        }
        if let Some(a) = self.agent_mut(kept) {
            a.wings += 1;
        }
        if let Some(a) = self.agent_mut(gone) {
            a.into = Some(kept);
        }
        Outcome::Changed
    }
}
