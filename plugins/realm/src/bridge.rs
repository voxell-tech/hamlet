//! The bridge from the agents service's roster to the society the
//! realm draws: every hamlet takes its town, every agent its plot and
//! every road its place, and what changes in the roster changes them.

use std::collections::{HashMap, HashSet};

use glam::Vec2;
use hamlet_engine_bevy::api::agents::{RoadUse, Roster, RosterAgent};
use hamlet_engine_bevy::prelude::*;

use crate::folk::keeper::KState;
use crate::society::{AgentId, Pair, RoadLevel, Society, TownId};
use crate::world::land::Land;

/// An agent as the roster names it: its hamlet and its number there.
pub type Who = (String, u64);

/// Something that befell an agent, for its robot to be seen doing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Moment {
    /// It left, merged into `into` when it has a host.
    Left {
        agent: AgentId,
        into: Option<AgentId>,
    },
    /// It came back.
    Back(AgentId),
}

/// What the realm knows of the roster.
#[derive(Resource, Default)]
pub struct Bridge {
    /// Each hamlet's town, by the hamlet's name on disk.
    towns: HashMap<String, TownId>,
    /// Each hamlet's spot as it was last set down.
    spots: HashMap<String, Option<(f32, f32)>>,
    /// The agent of the society each agent is drawn as.
    housed: HashMap<Who, AgentId>,
    /// Those that found no plot, until their hamlet has one.
    unhoused: HashSet<Who>,
    /// Those whose house was taken in by another as a wing.
    wings: HashMap<Who, AgentId>,
    /// Those that were gone when last seen.
    gone: HashSet<Who>,
    /// The roads laid, by their ends.
    roads: HashMap<[Who; 2], Pair>,
    /// Each agent of the society by its name and its hamlet.
    names: HashMap<AgentId, (String, String)>,
    /// The name each hamlet's chief gave it, by its name on disk.
    titles: HashMap<String, String>,
}

/// The bridge's methods on [`RosterAgent`].
trait RosterAgentExt {
    fn who(&self) -> Who;
}

impl RosterAgentExt for RosterAgent {
    fn who(&self) -> Who {
        (self.hamlet.clone(), self.number)
    }
}

/// The bridge's methods on [`RoadUse`].
trait RoadUseExt {
    /// The society's level of the use.
    fn level(self) -> RoadLevel;
}

impl RoadUseExt for RoadUse {
    fn level(self) -> RoadLevel {
        match self {
            RoadUse::Overgrown => RoadLevel::Overgrown,
            RoadUse::Dirt => RoadLevel::Dirt,
            RoadUse::Paved => RoadLevel::Paved,
            RoadUse::Rail => RoadLevel::Rail,
        }
    }
}

impl Bridge {
    /// The name and the hamlet of the agent `agent` is drawn for.
    pub fn named(&self, agent: AgentId) -> Option<&(String, String)> {
        self.names.get(&agent)
    }

    /// The agent of the society `who` is drawn as.
    pub fn housed(&self, who: &Who) -> Option<AgentId> {
        self.housed.get(who).copied()
    }

    /// The agent of the society the agent `name` is drawn as: the
    /// one of the latest, when an agent of that name lived before.
    pub fn find(&self, name: &str) -> Option<AgentId> {
        self.names
            .iter()
            .filter(|(_, (named, _))| named == name)
            .map(|(agent, _)| *agent)
            .max()
    }

    /// The hamlet whose town is `town`.
    pub fn hamlet(&self, town: TownId) -> Option<&str> {
        self.towns
            .iter()
            .find(|(_, of)| **of == town)
            .map(|(hamlet, _)| hamlet.as_str())
    }

    /// The name the chief gave the hamlet whose town is `town`.
    pub fn title(&self, town: TownId) -> Option<&str> {
        let hamlet = self.hamlet(town)?;
        self.titles.get(hamlet).map(String::as_str)
    }

    /// The town of the hamlet `hamlet`.
    pub fn town(&self, hamlet: &str) -> Option<TownId> {
        self.towns.get(hamlet).copied()
    }

    /// Brings the society in line with `roster`, and says what befell
    /// the agents it had met.
    pub fn follow(
        &mut self,
        roster: &Roster,
        land: &mut Land,
        society: &mut Society,
    ) -> Vec<Moment> {
        let mut moments = Vec::new();
        self.titles = roster
            .hamlets
            .iter()
            .map(|hamlet| (hamlet.id.clone(), hamlet.title.clone()))
            .collect();
        self.close(roster, society);
        self.move_out(roster, society);
        self.found(roster, land, society);
        self.resite(roster, land, society);
        self.turn(roster, land, society, &mut moments);
        self.house(roster, land, society);
        self.make_room(roster, land, society);
        self.lay(roster, society);
        self.show(roster, society);
        moments
    }

    /// A hamlet gone: its town goes with it, hall, common and all.
    fn close(&mut self, roster: &Roster, society: &mut Society) {
        let here = roster
            .hamlets
            .iter()
            .map(|hamlet| hamlet.id.as_str())
            .collect::<HashSet<_>>();
        self.towns.retain(|hamlet, town| {
            let stays = here.contains(hamlet.as_str());
            if !stays {
                society.remove_town(*town);
            }
            stays
        });
        self.spots
            .retain(|hamlet, _| here.contains(hamlet.as_str()));
    }

    /// An agent gone from its hamlet for good: it moved to another,
    /// or the hamlet was removed. Its house there is a ruin.
    fn move_out(&mut self, roster: &Roster, society: &mut Society) {
        let here = roster
            .agents
            .iter()
            .map(RosterAgent::who)
            .collect::<HashSet<_>>();
        let names = &mut self.names;
        self.housed.retain(|who, agent| {
            let stays = here.contains(who);
            if !stays {
                if society.agent(*agent).is_some_and(|a| !a.archived)
                {
                    society.remove_agent(*agent);
                }
                names.remove(agent);
            }
            stays
        });
        self.unhoused.retain(|who| here.contains(who));
        self.wings.retain(|who, _| here.contains(who));
        self.gone.retain(|who| here.contains(who));
    }

    /// Founds the town of each hamlet the realm has not met: at the
    /// spot its chief set it down, or on the first free site.
    fn found(
        &mut self,
        roster: &Roster,
        land: &mut Land,
        society: &mut Society,
    ) {
        for hamlet in &roster.hamlets {
            if self.towns.contains_key(&hamlet.id) {
                continue;
            }
            let town = match hamlet.spot {
                Some((x, z)) => {
                    society.found_town_at(land, Vec2::new(x, z))
                }
                None => society.found_town(land),
            };
            self.towns.insert(hamlet.id.clone(), town);
            self.spots.insert(hamlet.id.clone(), hamlet.spot);
        }
    }

    /// Sets each town down where its chief last put it. A place the
    /// ground cannot take is left out, and the town stays.
    fn resite(
        &mut self,
        roster: &Roster,
        land: &mut Land,
        society: &mut Society,
    ) {
        for hamlet in &roster.hamlets {
            let Some(town) = self.towns.get(&hamlet.id).copied()
            else {
                continue;
            };
            if self.spots.get(&hamlet.id) == Some(&hamlet.spot) {
                continue;
            }
            self.spots.insert(hamlet.id.clone(), hamlet.spot);
            let Some((x, z)) = hamlet.spot else {
                continue;
            };
            let to = Vec2::new(x, z);
            let stands = society
                .town(town)
                .map(|town| land.sites[town.site].at);
            if stands.is_none_or(|at| at == to) {
                continue;
            }
            if let Err(why) = society.move_town(land, town, to) {
                hamlet_engine_bevy::warn(&format!(
                    "the town of {} stays where it is: {to} is \
                     {why:?}",
                    hamlet.id
                ));
            }
        }
    }

    /// The host of a merged agent: the living agent of its hamlet it
    /// was merged into, if the realm has housed it.
    fn host(
        &self,
        roster: &Roster,
        gone: &RosterAgent,
    ) -> Option<AgentId> {
        let into = gone.merged_into.as_ref()?;
        let host = roster.agents.iter().find(|agent| {
            &agent.name == into
                && agent.hamlet == gone.hamlet
                && !agent.gone
        })?;
        self.housed.get(&host.who()).copied()
    }

    /// Agents the realm met that left or came back since.
    fn turn(
        &mut self,
        roster: &Roster,
        land: &Land,
        society: &mut Society,
        moments: &mut Vec<Moment>,
    ) {
        for agent in &roster.agents {
            let who = agent.who();
            let Some(drawn) = self.housed.get(&who).copied() else {
                continue;
            };
            let was = self.gone.contains(&who);
            if agent.gone && !was {
                let host = self.host(roster, agent);
                let host = self.archive(society, &who, drawn, host);
                moments.push(Moment::Left {
                    agent: drawn,
                    into: host,
                });
                self.gone.insert(who);
            } else if !agent.gone && was {
                match self.wings.remove(&who) {
                    Some(host) => {
                        society.split(land, host, drawn);
                    }
                    None => {
                        society.restore(land, drawn);
                    }
                }
                moments.push(Moment::Back(drawn));
                self.gone.remove(&who);
            }
        }
    }

    /// Archives `drawn` in the society: a wing on the house of
    /// `host`, or a ruin. The host, when there is a wing.
    fn archive(
        &mut self,
        society: &mut Society,
        who: &Who,
        drawn: AgentId,
        host: Option<AgentId>,
    ) -> Option<AgentId> {
        let host = host.filter(|host| society.merge(*host, drawn));
        match host {
            Some(host) => {
                self.wings.insert(who.clone(), host);
            }
            None => {
                society.remove_agent(drawn);
            }
        }
        host
    }

    /// Gives every agent the realm has not met a plot. Agents are
    /// housed, and those who left are seen out, in the order it
    /// happened, so that a newcomer takes the lowest plot free at
    /// the time, as it did when the realm last saw it.
    fn house(
        &mut self,
        roster: &Roster,
        land: &Land,
        society: &mut Society,
    ) {
        for hamlet in &roster.hamlets {
            let Some(town) = self.towns.get(&hamlet.id).copied()
            else {
                continue;
            };
            let Some(leader) = society.town(town).map(|t| t.leader)
            else {
                continue;
            };
            let new = roster
                .agents
                .iter()
                .filter(|agent| agent.hamlet == hamlet.id)
                .filter(|agent| {
                    let who = agent.who();
                    !self.housed.contains_key(&who)
                        && !self.unhoused.contains(&who)
                })
                .collect::<Vec<_>>();
            if new.is_empty() {
                continue;
            }
            let mut story = new
                .iter()
                .flat_map(|agent| {
                    let came = (agent.since, agent.number, false);
                    let went = agent.gone.then(|| {
                        (
                            agent.until.unwrap_or(u64::MAX),
                            agent.number,
                            true,
                        )
                    });
                    core::iter::once(came)
                        .chain(went)
                        .map(move |when| (when, *agent))
                })
                .collect::<Vec<_>>();
            story.sort_by_key(|(when, _)| *when);
            for ((_, number, goes), agent) in story {
                let who = agent.who();
                if !goes {
                    // The hall is the first mayor's house, whoever
                    // holds the office.
                    let drawn = if number == 0 {
                        Some(leader)
                    } else if society.has_room(land, town) {
                        society.add_agent(land, town)
                    } else {
                        None
                    };
                    match drawn {
                        Some(drawn) => {
                            self.housed.insert(who, drawn);
                            self.names.insert(
                                drawn,
                                (
                                    agent.name.clone(),
                                    agent.hamlet.clone(),
                                ),
                            );
                        }
                        None => {
                            hamlet_engine_bevy::warn(&format!(
                                "no plot is left for {}, which has \
                                 no house until one is free",
                                agent.name
                            ));
                            self.unhoused.insert(who);
                        }
                    }
                    continue;
                }
                // Those who left before the realm met them are not
                // seen going.
                let Some(drawn) = self.housed.get(&who).copied()
                else {
                    continue;
                };
                let host = self.host(roster, agent);
                self.archive(society, &who, drawn, host);
                self.gone.insert(who);
            }
        }
    }

    /// Lets an agent that found no plot try again once its hamlet has
    /// one.
    fn make_room(
        &mut self,
        roster: &Roster,
        land: &Land,
        society: &mut Society,
    ) {
        let towns = &self.towns;
        let before = self.unhoused.len();
        self.unhoused.retain(|(hamlet, _)| {
            !towns
                .get(hamlet)
                .is_some_and(|town| society.has_room(land, *town))
        });
        if self.unhoused.len() != before {
            self.house(roster, land, society);
        }
    }

    /// Lays every road the realm has not met, follows the use of
    /// those it has, and takes up those that went.
    fn lay(&mut self, roster: &Roster, society: &mut Society) {
        let mut here = HashSet::new();
        for road in &roster.roads {
            let (Some(a), Some(b)) = (
                self.housed.get(&road.ends[0]).copied(),
                self.housed.get(&road.ends[1]).copied(),
            ) else {
                continue;
            };
            let pair = Pair::new(a, b);
            if society.road(pair).is_none() {
                society.add_road(a, b);
            }
            let level = road.level.level();
            if society.road(pair).is_some_and(|r| r.level != level) {
                society.set_level(pair, level);
            }
            self.roads.insert(road.ends.clone(), pair);
            here.insert(road.ends.clone());
        }
        self.roads.retain(|ends, pair| {
            let stays = here.contains(ends);
            if !stays && society.road(*pair).is_some() {
                society.roads.retain(|r| r.pair != *pair);
            }
            stays
        });
    }

    /// Shows what each agent is doing. Waiting for the chief shows
    /// over whatever else it does.
    fn show(&self, roster: &Roster, society: &mut Society) {
        for agent in roster.agents.iter().filter(|agent| !agent.gone)
        {
            let Some(drawn) = self.housed.get(&agent.who()).copied()
            else {
                continue;
            };
            let state = if agent.waiting > 0 {
                KState::Waiting
            } else if agent.busy {
                KState::Working
            } else {
                KState::Rest
            };
            if society.agent(drawn).is_some_and(|a| a.state != state)
            {
                society.set_state(drawn, state);
            }
            if society
                .agent(drawn)
                .is_some_and(|a| a.work != agent.doing)
            {
                society.set_work(drawn, agent.doing);
            }
        }
    }
}
