//! The society the world is drawn from: towns, agents and the roads
//! between them.

mod places;

use std::hash::{DefaultHasher, Hash, Hasher};

use hamlet_engine_sdk::api::agents::Doing;

pub use self::places::{Claim, Place};
use crate::house::{HouseSettings, Remains};
use crate::keeper::KState;
use crate::land::Land;
use crate::layout::plot;

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
    pub leader: AgentId,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Agent {
    pub id: AgentId,
    pub town: TownId,
    /// The plot its house stands on. Changes only when it moves
    /// town.
    pub plot: u32,
    pub state: KState,
    /// Its step while it works.
    pub work: Doing,
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

    pub fn has(&self, id: AgentId) -> bool {
        self.0 == id || self.1 == id
    }
}

#[derive(Clone, Debug, PartialEq, Hash)]
pub struct Road {
    pub pair: Pair,
    pub level: RoadLevel,
}

/// Everything the world shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Society {
    pub towns: Vec<Town>,
    pub agents: Vec<Agent>,
    pub roads: Vec<Road>,
    /// Ground no town may stand on.
    pub claims: Vec<Claim>,
    next_agent: u32,
    next_town: u32,
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
    pub fn citizens(
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
            state: KState::Rest,
            work: Doing::default(),
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
                && !self.citizens(town.id).any(|a| a.plot == k)
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

    /// Adds an agent with a house and a road to its leader. None
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
        self.add_road(id, t.leader);
        Some(id)
    }

    /// Lays a dirt road, unless the two already have one.
    pub fn add_road(&mut self, a: AgentId, b: AgentId) -> bool {
        let pair = Pair::new(a, b);
        let live = |id| self.agent(id).is_some_and(|a| !a.archived);
        if a == b || !live(a) || !live(b) || self.road(pair).is_some()
        {
            return false;
        }
        self.roads.push(Road {
            pair,
            level: RoadLevel::Dirt,
        });
        true
    }

    pub fn set_level(&mut self, pair: Pair, level: RoadLevel) {
        if let Some(r) =
            self.roads.iter_mut().find(|r| r.pair == pair)
        {
            r.level = level;
        }
    }

    pub fn set_state(&mut self, id: AgentId, state: KState) {
        if let Some(a) = self.agent_mut(id) {
            a.state = state;
        }
    }

    pub fn set_work(&mut self, id: AgentId, work: Doing) {
        if let Some(a) = self.agent_mut(id) {
            a.work = work;
        }
    }

    /// Takes a town out of the world with everyone in it, its hall
    /// and the roads to it. Its site is free to be founded on again.
    pub(crate) fn remove_town(&mut self, id: TownId) -> bool {
        if self.town(id).is_none() {
            return false;
        }
        let gone = self
            .agents
            .iter()
            .filter(|a| a.town == id)
            .map(|a| a.id)
            .collect::<Vec<_>>();
        self.roads
            .retain(|r| !gone.iter().any(|agent| r.pair.has(*agent)));
        self.agents.retain(|a| a.town != id);
        self.towns.retain(|t| t.id != id);
        true
    }

    /// Archives an agent: its house becomes a ruin and its roads go.
    /// A leader cannot be removed.
    pub fn remove_agent(&mut self, id: AgentId) -> bool {
        if self.towns.iter().any(|t| t.leader == id) {
            return false;
        }
        let Some(agent) = self.agent_mut(id) else {
            return false;
        };
        agent.archived = true;
        self.roads.retain(|r| !r.pair.has(id));
        true
    }

    /// Brings an archived agent back, with a road to its leader.
    /// Its house stands where it stood, or on another plot if a
    /// newcomer has built there since. False when its town has no
    /// plot left for it.
    pub fn restore(&mut self, land: &Land, id: AgentId) -> bool {
        let Some(agent) = self.agent(id).cloned() else {
            return false;
        };
        let Some(town) = self.town(agent.town).cloned() else {
            return false;
        };
        let taken =
            self.citizens(town.id).any(|a| a.plot == agent.plot);
        let plot = if taken || agent.cleared {
            match self.free_plot(land, &town) {
                Some(plot) => plot,
                None => return false,
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
        self.add_road(id, town.leader);
        true
    }

    /// Undoes a merge: `gone` comes back and `kept` loses the wing.
    /// The roads stay as they are.
    pub fn split(
        &mut self,
        land: &Land,
        kept: AgentId,
        gone: AgentId,
    ) -> bool {
        if !self.restore(land, gone) {
            return false;
        }
        if let Some(a) = self.agent_mut(kept) {
            a.wings = a.wings.saturating_sub(1);
        }
        true
    }

    /// Merges `gone` into `kept`: `kept` gains a wing and the roads
    /// of both, and `gone`'s house is archived.
    pub fn merge(&mut self, kept: AgentId, gone: AgentId) -> bool {
        let leads = |id| self.towns.iter().any(|t| t.leader == id);
        if kept == gone || leads(gone) {
            return false;
        }
        let moved = self
            .roads
            .iter()
            .filter(|r| r.pair.has(gone) && !r.pair.has(kept))
            .map(|r| {
                let other = if r.pair.0 == gone {
                    r.pair.1
                } else {
                    r.pair.0
                };
                (Pair::new(kept, other), r.level)
            })
            .collect::<Vec<_>>();
        if !self.remove_agent(gone) {
            return false;
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
        true
    }
}
