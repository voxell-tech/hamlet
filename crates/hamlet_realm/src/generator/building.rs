//! Houses, roads and commons built in background tasks, so that a
//! change to many of them at once does not hold a frame up.

use std::collections::HashMap;

use bevy::mesh::Indices;
use bevy::prelude::*;
use bevy::tasks::futures_lite::future::poll_once;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on};
use hamlet_vfx::flag::{FlagSettings, cloth};

use super::house::{self, Remains};
use super::land::Land;
use super::layout::{End, Route};
use super::shift::Way;
use super::society::{AgentId, Pair, RoadLevel};
use super::yard::Yard;
use super::{common, road};
use crate::meshes::Mb;

/// Triangles and bytes of mesh data.
#[derive(Clone, Copy, Default)]
pub(super) struct Cost {
    pub tris: usize,
    pub bytes: usize,
}

impl Cost {
    fn of(parts: &[&Mb]) -> Self {
        Self {
            tris: parts.iter().map(|mb| mb.tris()).sum(),
            bytes: parts.iter().map(|mb| mb.bytes()).sum(),
        }
    }
}

pub(super) fn mesh_bytes(mesh: &Mesh) -> usize {
    mesh.count_vertices() * mesh.get_vertex_size() as usize
        + mesh.indices().map_or(0, |i| match i {
            Indices::U16(i) => i.len() * 2,
            Indices::U32(i) => i.len() * 4,
        })
}

/// The makings of a house's mesh, besides its agent.
#[derive(Clone, Copy)]
pub(super) struct HouseRecipe {
    pub agent: AgentId,
    pub site: usize,
    pub hall: bool,
    pub leader: bool,
    /// Its remains, if its agent is archived.
    pub remains: Option<Remains>,
    pub wings: u8,
    pub yard: Yard,
}

pub(super) struct HouseBuilt {
    pub body: Mesh,
    /// The window panes and where the door lantern hangs. Only a
    /// house that is lived in has them.
    pub fittings: Option<(Mesh, Vec3)>,
    /// The leader's banner, and where it hangs from.
    pub banner: Option<(Mesh, Vec3)>,
    pub cost: Cost,
}

fn build_house(land: &Land, recipe: HouseRecipe) -> HouseBuilt {
    if let Some(remains) = recipe.remains {
        let rest = match remains {
            Remains::Ruin => {
                house::ruin(land, recipe.agent, recipe.hall)
            }
            Remains::Bare => Mb::new(),
        };
        return HouseBuilt {
            cost: Cost::of(&[&rest]),
            body: rest.build(),
            fittings: None,
            banner: None,
        };
    }
    let built = house::house(
        land,
        recipe.agent,
        &house::style_of(land, recipe.site),
        recipe.hall,
        recipe.leader,
        recipe.wings,
        &recipe.yard,
    );
    HouseBuilt {
        cost: Cost::of(&[&built.body, &built.panes]),
        body: built.body.build(),
        fittings: Some((built.panes.build(), built.lamp)),
        banner: built.banner.map(|(at, colours)| {
            let cloth = cloth(
                &FlagSettings::default(),
                house::BANNER.x,
                house::BANNER.y,
                &colours,
            );
            (cloth, at)
        }),
    }
}

/// The makings of a road's line: its ends, and the places of the
/// towns it runs between.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) struct Course {
    pub ends: (End, End),
    /// The places of the towns of its two ends.
    at: (Vec2, Vec2),
    /// A number for the bridges of other roads that its own bridges
    /// meet. Zero for none.
    pub meets: u64,
}

impl Course {
    pub fn of(land: &Land, ends: (End, End)) -> Self {
        Self {
            ends,
            at: (
                land.sites[ends.0.site].at,
                land.sites[ends.1.site].at,
            ),
            meets: 0,
        }
    }

    /// True when it runs from one town to another.
    pub fn between(&self) -> bool {
        self.ends.0.site != self.ends.1.site
    }
}

pub(super) struct RoadBuilt {
    pub route: Route,
    pub surface: Mesh,
    pub groove: Mesh,
    pub cost: Cost,
    /// True when any of it stands high enough to throw a shadow.
    pub bridge: bool,
    /// Its bridges, for the roads that meet them.
    pub span: Option<road::Span>,
}

fn build_road(
    land: &Land,
    pair: Pair,
    level: RoadLevel,
    ends: (End, End),
    met: &[road::Span],
) -> RoadBuilt {
    let route = Route::between(land, ends.0, ends.1);
    let salt = ((pair.0.0 as u64) << 32) | pair.1.0 as u64;
    let surface = road::surface(land, &route, level, salt, met);
    let groove = road::groove(land, &route, level, salt);
    let bridge =
        route.raised.iter().any(|&h| h > land.set.road.bridge_from);
    RoadBuilt {
        cost: Cost {
            tris: surface.tris(),
            bytes: surface.bytes() + mesh_bytes(&groove),
        },
        surface: surface.build(),
        groove,
        span: road::Span::of(land, &route),
        route,
        bridge,
    }
}

pub(super) struct HouseJob {
    pub agent: AgentId,
    /// The entity that waits for the mesh.
    pub entity: Entity,
    /// The house it replaces, drawn until this one is ready.
    pub old: Option<Entity>,
    /// Its coming in or going out. None when it only takes the place
    /// of the old one.
    pub way: Option<Way>,
    /// Seconds before what it replaces goes, and before it comes.
    pub wait: (f32, f32),
    task: Task<HouseBuilt>,
}

pub(super) struct RoadJob {
    pub level: RoadLevel,
    pub course: Course,
    task: Task<RoadBuilt>,
}

/// Meshes being built, each for something the society already holds.
#[derive(Default)]
pub(super) struct Builds {
    houses: Vec<HouseJob>,
    roads: HashMap<Pair, RoadJob>,
    commons: Vec<(Entity, Task<Mesh>)>,
}

impl Builds {
    pub fn is_empty(&self) -> bool {
        self.houses.is_empty()
            && self.roads.is_empty()
            && self.commons.is_empty()
    }

    pub fn house(
        &mut self,
        land: &Land,
        recipe: HouseRecipe,
        entity: Entity,
        old: Option<Entity>,
        way: Option<Way>,
        wait: (f32, f32),
    ) {
        let land = land.clone();
        let task = AsyncComputeTaskPool::get()
            .spawn(async move { build_house(&land, recipe) });
        self.houses.push(HouseJob {
            agent: recipe.agent,
            entity,
            old,
            way,
            wait,
            task,
        });
    }

    /// True when `agent`'s house is being built with nothing drawn in
    /// its place meanwhile.
    pub(crate) fn bare_house(&self, agent: AgentId) -> bool {
        self.houses.iter().any(|job| {
            job.agent == agent
                && job.old.is_none()
                && job.way.is_none()
        })
    }

    /// True when the common `entity` is still being built.
    pub(crate) fn bare_common(&self, entity: Entity) -> bool {
        self.commons.iter().any(|(common, _)| *common == entity)
    }

    /// Forgets the house being built for `entity` and returns the one
    /// it was to replace.
    pub(crate) fn cancel_house(
        &mut self,
        entity: Entity,
    ) -> Option<Entity> {
        let at =
            self.houses.iter().position(|j| j.entity == entity)?;
        self.houses.swap_remove(at).old
    }

    /// The road being built for `pair`, if any.
    pub fn road(&self, pair: Pair) -> Option<&RoadJob> {
        self.roads.get(&pair)
    }

    /// Starts building a road, in place of any being built for the
    /// same pair. `met` is the bridges of the other roads.
    pub fn lay_road(
        &mut self,
        land: &Land,
        pair: Pair,
        level: RoadLevel,
        course: Course,
        met: Vec<road::Span>,
    ) {
        let land = land.clone();
        let task = AsyncComputeTaskPool::get().spawn(async move {
            build_road(&land, pair, level, course.ends, &met)
        });
        self.roads.insert(
            pair,
            RoadJob {
                level,
                course,
                task,
            },
        );
    }

    /// Forgets the roads being built that `keep` rejects.
    pub fn keep_roads(&mut self, keep: impl Fn(&Pair) -> bool) {
        self.roads.retain(|pair, _| keep(pair));
    }

    pub fn common(
        &mut self,
        land: &Land,
        site: usize,
        entity: Entity,
    ) {
        let land = land.clone();
        let task = AsyncComputeTaskPool::get().spawn(async move {
            let style = house::style_of(&land, site);
            common::mesh(&land, site, &style).build()
        });
        self.commons.push((entity, task));
    }

    /// Hands over what has finished, at most `most` things.
    pub fn finished(&mut self, most: usize) -> Finished {
        let mut out = Finished::default();
        let mut left = most;
        self.commons.retain_mut(|(entity, task)| {
            if left == 0 {
                return true;
            }
            let Some(mesh) = block_on(poll_once(task)) else {
                return true;
            };
            left -= 1;
            out.commons.push((*entity, mesh));
            false
        });
        let mut waiting = Vec::new();
        for mut job in self.houses.drain(..) {
            let built = (left > 0)
                .then(|| block_on(poll_once(&mut job.task)))
                .flatten();
            match built {
                Some(built) => {
                    left -= 1;
                    out.houses.push((job, built));
                }
                None => waiting.push(job),
            }
        }
        self.houses = waiting;
        self.roads.retain(|pair, job| {
            if left == 0 {
                return true;
            }
            let Some(built) = block_on(poll_once(&mut job.task))
            else {
                return true;
            };
            left -= 1;
            out.roads.push((*pair, job.level, job.course, built));
            false
        });
        out
    }
}

/// Meshes ready to be put into the world.
#[derive(Default)]
pub(super) struct Finished {
    pub commons: Vec<(Entity, Mesh)>,
    pub houses: Vec<(HouseJob, HouseBuilt)>,
    pub roads: Vec<(Pair, RoadLevel, Course, RoadBuilt)>,
}
