//! Keeping what is spawned in line with the society.

mod houses;
mod roads;
mod towns;

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;

use super::building::{Builds, Cost, Course};
use super::land::Land;
use super::layout::Route;
use super::lights::Lamp;
use super::loading::Loading;
use super::society::{AgentId, Pair, RoadLevel, Society, TownId};
use super::work::{Engine, Passage, Robot};
use super::{GenOptions, HouseMotion, Kit, Tally};
use crate::Clock;
use crate::keeper::{self, KState};
use crate::sky::SkySettings;

#[derive(Clone, PartialEq)]
pub(super) struct HouseSig {
    pub(super) site: usize,
    /// The place of its town.
    pub(super) at: Vec2,
    pub(super) plot: u32,
    pub(super) archived: bool,
    pub(super) cleared: bool,
    pub(super) leader: bool,
    pub(super) wings: u8,
}

pub(super) struct RoadDrawn {
    pub(super) entity: Entity,
    pub(super) groove: Entity,
    pub(super) level: RoadLevel,
    pub(super) course: Course,
    pub(super) cost: Cost,
    pub(super) route: Route,
    /// Its bridges, for the roads that meet them.
    pub(super) span: Option<super::road::Span>,
    /// The agent at the route's start.
    pub(super) first: AgentId,
}

pub(super) struct TownDrawn {
    pub(super) site: usize,
    /// Its place when it was last drawn.
    pub(super) at: Vec2,
    pub(super) common: Entity,
    /// Parent of the houses and the common.
    pub(super) near: Entity,
    pub(super) far: Entity,
    pub(super) far_panes: Entity,
}

/// The things spawned for the society as last seen.
#[derive(Resource, Default)]
pub(super) struct Drawn {
    pub(super) towns: HashMap<TownId, TownDrawn>,
    pub(super) houses: HashMap<AgentId, (Entity, HouseSig, Cost)>,
    pub(super) lamps: HashMap<AgentId, (Entity, KState)>,
    pub(super) engines: HashMap<AgentId, Engine>,
    pub(super) robots: HashMap<AgentId, Entity>,
    pub(super) roads: HashMap<Pair, RoadDrawn>,
    /// The place of each site when the roads were last looked over,
    /// and the ground held then.
    sites: Vec<Vec2>,
    holds: Vec<(Vec2, f32)>,
    /// The society's shape when it was last brought in line. None
    /// when its next change has it looked over whatever its shape.
    pub(super) shape: Option<u64>,
}

/// True when the society changed in what it would have built.
pub(super) fn reshaped(
    society: Res<Society>,
    drawn: Res<Drawn>,
) -> bool {
    society.is_changed() && drawn.shape != Some(society.shape())
}

/// Shows what each agent is doing: in its door lantern, and to its
/// robot.
pub(super) fn tell_states(
    mut commands: Commands,
    society: Res<Society>,
    mut drawn: ResMut<Drawn>,
    mut robots: Query<&mut Robot>,
) {
    for agent in &society.agents {
        let shown = drawn.lamps.get(&agent.id).map(|lamp| lamp.1);
        if shown.is_some_and(|state| state != agent.state)
            && let Some((lamp, state)) =
                drawn.lamps.get_mut(&agent.id)
        {
            *state = agent.state;
            commands.entity(*lamp).try_insert(Lamp(agent.state));
        }
        if let Some(robot) = drawn.robots.get(&agent.id)
            && let Ok(mut robot) = robots.get_mut(*robot)
            && (robot.state != agent.state
                || robot.work != agent.work)
        {
            robot.state = agent.state;
            robot.work = agent.work;
        }
    }
}

/// One pass over the society: what its steps work on, and what
/// they tell each other.
struct Pass<'a, 'w, 's> {
    commands: &'a mut Commands<'w, 's>,
    society: &'a Society,
    land: &'a Land,
    kit: &'a Kit,
    drawn: &'a mut Drawn,
    builds: &'a mut Builds,
    motion: &'a HouseMotion,
    /// True when the clock is held, so that nothing comes or goes in
    /// steps.
    frozen: bool,
    /// True when robots are drawn.
    robots: bool,
    robot_kit: &'a keeper::Kit,
    materials: &'a mut Assets<StandardMaterial>,
    passing: &'a Query<'a, 'a, &'static Passage>,
    /// Towns whose far mesh is to be built anew.
    stale: HashSet<TownId>,
    /// Towns that were picked up and set down elsewhere.
    carried: HashSet<TownId>,
    /// Towns first drawn in this pass. Their houses are there from
    /// the start.
    founded: HashSet<TownId>,
}

impl Pass<'_, '_, '_> {
    /// Has the far mesh of every town on `site` built anew.
    fn stale_on(&mut self, site: usize) {
        let on = self.society.towns.iter().filter(|t| t.site == site);
        self.stale.extend(on.map(|t| t.id));
    }
}

/// Brings what is spawned in line with the society: spawns what is
/// new, rebuilds what changed and despawns what is gone. What each
/// agent is doing is left to [`tell_states`].
pub(super) fn sync(
    mut commands: Commands,
    society: Res<Society>,
    land: Res<Land>,
    kit: Res<Kit>,
    sky: Res<SkySettings>,
    options: Res<GenOptions>,
    clock: Res<Clock>,
    mut drawn: ResMut<Drawn>,
    mut loading: ResMut<Loading>,
    mut tally: ResMut<Tally>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    passing: Query<&'static Passage>,
    robot_kit: Res<keeper::Kit>,
    motion: Res<HouseMotion>,
) {
    let mut pass = Pass {
        commands: &mut commands,
        society: &society,
        land: &land,
        kit: &kit,
        drawn: &mut drawn,
        builds: &mut loading.builds,
        motion: &motion,
        frozen: clock.frozen,
        robots: options.robots,
        robot_kit: &robot_kit,
        materials: &mut materials,
        passing: &passing,
        stale: HashSet::new(),
        carried: HashSet::new(),
        founded: HashSet::new(),
    };
    pass.clear();
    pass.towns(&sky);
    pass.houses();
    pass.roads();
    pass.far(&mut meshes);
    drawn.shape = Some(society.shape());
    tally.count(&society, &drawn);
}
