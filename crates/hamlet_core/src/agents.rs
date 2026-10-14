//! The hamlets as entities: one for each hamlet, agent and road, kept
//! in step with the records on disk by [`sync()`] and [`account`].

mod activity;
mod roster;
mod sync;

use std::path::PathBuf;

use bevy_app::{App, Plugin, Update};
use bevy_ecs::name::Name;
use bevy_ecs::prelude::*;
use hamlet_home::limits::Limits;
use hamlet_rules::budget::{AgentBudget, Report};
use hamlet_rules::hamlet::{HOME, Hamlet, RoadUse};
use hamlet_rules::library::Library;
use hamlet_rules::realm::Realm;
use hamlet_rules::work::Work;

pub use self::activity::{Activity, Doing, Paper, Saying};
pub(crate) use self::roster::ask;
pub use self::roster::{
    Roster, hamlet_of, mayor, mayor_of, name_of, named,
};
use self::sync::found;
pub(crate) use self::sync::watch;
pub use self::sync::{Seen, WatchEvery, account, sync};
use crate::core::HomeDir;

/// Brings the hamlets of `home` up as entities and keeps them in
/// step with their files.
pub struct AgentsPlugin {
    pub home: PathBuf,
}
impl Plugin for AgentsPlugin {
    fn build(&self, app: &mut App) {
        // A home from before there were several hamlets becomes the
        // first.
        if let Err(e) = Realm::open(&self.home) {
            eprintln!("hamlet: could not open the realm: {e:#}");
        }
        if let Err(e) = Library::open(&self.home) {
            eprintln!("hamlet: could not open the library: {e:#}");
        }
        register(app.world_mut());
        found(app.world_mut(), &self.home, HOME);
        sync(app.world_mut());
        app.add_systems(Update, watch);
    }
}

/// A hamlet, by its name in the home it lives under.
#[derive(Component, Clone, Debug)]
#[component(immutable)]
#[require(Seen)]
pub struct Settlement {
    pub home: PathBuf,
    pub name: String,
}

impl Settlement {
    /// The hamlet's record on disk.
    pub fn record(&self) -> Hamlet {
        Hamlet::at(&self.home, &self.name)
    }
}

/// A hamlet's limits as its chief wrote them. Nothing while the
/// file cannot be read.
#[derive(Component, Clone, PartialEq, Debug)]
#[component(immutable)]
pub struct Charter(pub Option<Limits>);

/// The first hamlet.
#[derive(Resource, Clone, Copy, Debug)]
struct First(Entity);

/// The limits of the first hamlet as they were last read. `Err`
/// when they cannot be read.
pub(crate) fn limits(world: &World) -> anyhow::Result<Limits> {
    let read = world
        .get_resource::<First>()
        .and_then(|first| world.get::<Charter>(first.0));
    match read {
        Some(Charter(Some(limits))) => Ok(limits.clone()),
        Some(Charter(None)) => {
            anyhow::bail!("the limits cannot be read")
        }
        // No hamlet has been brought up as an entity.
        None => match world.get_resource::<HomeDir>() {
            Some(home) => Limits::load(&home.0.root),
            None => anyhow::bail!("there is no hamlet"),
        },
    }
}

/// A hamlet's folder and mode, as its record last says.
#[derive(Component, Clone, PartialEq, Eq, Debug, Default)]
#[component(immutable)]
pub struct Works(pub Work);

/// The place a hamlet's chief set its town down, as x and z. None
/// while it stands where the world first put it.
#[derive(Component, Clone, Copy, PartialEq, Debug, Default)]
pub struct Spot(pub Option<(f32, f32)>);

/// An agent, by its number in the hamlet's record. The number is
/// kept for life; a later agent of the same name has another.
#[derive(Component, Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[component(immutable)]
#[require(Activity, Lived)]
pub struct Citizen(pub u64);

/// The times an agent was created and, if it has left, left, in
/// milliseconds as the record has them.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Lived {
    pub since: u64,
    pub until: Option<u64>,
}

/// The hamlet an agent belongs to.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
#[relationship(relationship_target = Citizens)]
pub struct LivesIn(pub Entity);

/// The agents of a hamlet, those that left included.
#[derive(Component, Default, Debug)]
#[relationship_target(relationship = LivesIn, linked_spawn)]
pub struct Citizens(Vec<Entity>);

/// The agent holds the mayor's office in its hamlet.
#[derive(Component, Clone, Copy, Debug)]
#[component(immutable)]
pub struct Mayor;

/// An agent's purpose, as the record has it.
#[derive(Component, Clone, PartialEq, Eq, Debug)]
#[component(immutable)]
pub struct Purpose(pub String);

/// The agent has a turn running, or another agent waits on it.
#[derive(Component, Clone, Copy, Debug)]
pub struct Busy;

/// The agent was removed or merged away and has not come back.
#[derive(Component, Clone, Copy, Debug)]
#[component(immutable)]
pub struct Left;

/// The agent this one was merged into.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
#[relationship(relationship_target = Absorbed)]
pub struct MergedInto(pub Entity);

/// The agents merged into this one.
#[derive(Component, Default, Debug)]
#[relationship_target(relationship = MergedInto)]
pub struct Absorbed(Vec<Entity>);

/// An agent's model and its spend in the period.
#[derive(Component, Clone, PartialEq, Debug)]
#[component(immutable)]
pub struct Account(pub AgentBudget);

/// A hamlet's spend against its budget. Each agent's line is its
/// [`Account`].
#[derive(Component, Clone, PartialEq, Debug, Default)]
#[component(immutable)]
pub struct Treasury(pub Report);

/// A road and the two agents it joins.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
#[component(immutable)]
pub struct Road {
    pub ends: (Entity, Entity),
}

impl Road {
    /// True when `agent` is at one of its ends.
    pub fn has(&self, agent: Entity) -> bool {
        self.ends.0 == agent || self.ends.1 == agent
    }
}

/// A road's use.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
#[component(immutable)]
pub struct Traffic {
    pub level: RoadUse,
    pub messages: u32,
    /// The sender of the latest message along it.
    pub last_from: Option<Entity>,
}

/// A message went along `road` since the entities were last brought
/// in step.
#[derive(Event, Clone, Copy, Debug)]
pub struct Carried {
    pub road: Entity,
    pub from: Entity,
}

/// The entities were brought in step with the record.
#[derive(Event, Clone, Copy, Debug)]
pub(crate) struct Synced;

/// An agent called on the librarian: `agent` when the world knows
/// which.
#[derive(Event, Clone, Copy, Debug)]
pub struct Consulted {
    pub agent: Option<Entity>,
}

/// `agent` cast a ballot.
#[derive(Event, Clone, Copy, Debug)]
pub struct Voted {
    pub agent: Entity,
}

/// Makes every component of an agent known to `world`, so a query
/// for one can be made from a shared reference before any agent
/// has it.
pub fn register(world: &mut World) {
    world.register_component::<Citizen>();
    world.register_component::<Name>();
    world.register_component::<Purpose>();
    world.register_component::<Mayor>();
    world.register_component::<Busy>();
    world.register_component::<Left>();
    world.register_component::<MergedInto>();
    world.register_component::<Account>();
}

#[cfg(test)]
mod tests;
