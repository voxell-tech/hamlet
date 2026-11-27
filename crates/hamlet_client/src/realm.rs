//! The hamlets and agents as the server last sent them.

pub(crate) mod received;

use bevy_ecs::lifecycle::HookContext;
use bevy_ecs::name::Name;
use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemParam;
use bevy_ecs::world::DeferredWorld;
use hamlet_protocol::client as protocol;
pub use hamlet_protocol::{HAND, MAYOR};

use crate::index::{self, Index};

/// The realm's folder on the server machine, excluded from local
/// chat links.
#[derive(Resource, Clone, Debug, PartialEq, Eq)]
pub struct RealmFolder(pub String);

/// The Hand, the realm's own agent.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct HandId(pub protocol::AgentId);

/// The realm's own limits.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct RealmBylaws(pub protocol::RealmLimits);

impl Default for RealmBylaws {
    fn default() -> Self {
        Self(protocol::RealmLimits {
            budget: protocol::LimitsBudget {
                cost: None,
                minutes: None,
                period: protocol::Period::Day,
            },
            roads_between: true,
        })
    }
}

/// The received mode for edits and commands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Manual,
    Auto,
}

impl From<protocol::HamletMode> for Mode {
    fn from(mode: protocol::HamletMode) -> Self {
        match mode {
            protocol::HamletMode::Manual => Self::Manual,
            protocol::HamletMode::Auto => Self::Auto,
        }
    }
}

/// A hamlet's protocol id, which is not its shown `Name`.
#[derive(Component, Clone, Debug, PartialEq)]
#[component(
    immutable,
    on_insert = index::file_hamlet,
    on_discard = index::unfile_hamlet
)]
#[require(Spot)]
pub struct HamletId(pub String);

/// The hamlet's folder, earlier folders, mode words and size.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct Works {
    pub folder: Option<String>,
    pub before: Vec<String>,
    pub mode: Mode,
    pub words: String,
    /// The most hires it holds.
    pub tier: u64,
}

/// The chief's received limits for a hamlet.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct Bylaws(pub protocol::Limits);

/// The server's ready-made budget report.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct Treasury(pub protocol::Report);

/// An agent's protocol id, which every message names it by.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[component(
    immutable,
    on_insert = index::file_agent,
    on_discard = index::unfile_agent
)]
#[require(Activity, Lived, Kind)]
pub struct Identity(pub protocol::AgentId);

/// Whether an agent is the Hand, a hire or a worker.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Kind(pub protocol::AgentKind);

impl Default for Kind {
    fn default() -> Self {
        Self(protocol::AgentKind::Hire)
    }
}

/// An agent that is a hire, for queries that want only hires.
#[derive(Component, Clone, Copy, Debug)]
pub struct Hire;

/// The agent a hire or worker answers to.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Parent(pub protocol::AgentId);

/// The profile a hire is of.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct OfProfile(pub protocol::ProfileId);

/// What a hire is called in the world.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct Nickname(pub String);

/// What an agent is shown by: a hire's nickname, else its profile's
/// name, else its address.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct Shown(pub String);

/// The version of its profile a hire is pinned to.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct Pin(pub String);

/// The dollars a hire may spend in its hamlet's budget period.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Allowance(pub f64);

/// The model chosen for an agent, while it has one.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct OwnModel(pub String);

/// How hard an agent thinks when chosen for it, while it has one.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct OwnEffort(pub hamlet_protocol::Effort);

/// An agent whose calls ask for the provider's fast tier.
#[derive(Component, Clone, Copy, Debug)]
pub struct Fast;

/// A profile of the city, with its `Name`.
#[derive(Component, Clone, Debug, PartialEq)]
#[component(
    immutable,
    on_insert = index::file_profile,
    on_discard = index::unfile_profile
)]
pub struct Profile(pub protocol::Profile);

/// The agent's purpose as sent by the server.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct Purpose(pub String);

/// The times an agent arrived and left, in epoch milliseconds.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Lived {
    pub since: u64,
    pub until: Option<u64>,
}

/// The hamlet an agent belongs to.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[component(
    on_insert = index::file_agent,
    on_discard = index::unlive
)]
#[relationship(relationship_target = Agents)]
pub struct LivesIn(pub Entity);

/// The agents belonging to a hamlet.
#[derive(Component, Default, Debug)]
#[relationship_target(relationship = LivesIn, linked_spawn)]
pub struct Agents(Vec<Entity>);

/// The agent holding its hamlet's office.
#[derive(Component, Clone, Copy, Debug)]
#[component(on_insert = index::file_agent, on_discard = index::unseat)]
pub struct Mayor;

/// An agent with running or queued work.
#[derive(Component, Clone, Copy, Debug)]
pub struct Busy;

/// An agent whose last work failed.
#[derive(Component, Clone, Copy, Debug)]
pub struct Faulted;

/// The kind of paper an agent reads or writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Paper {
    File,
    Memory,
    Skill,
}

/// The activity shown by an agent's robot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Doing {
    #[default]
    Unknown,
    Think,
    Read(Paper),
    Write(Paper),
    Run,
    Fetch,
    CustomTool,
    Await,
}

impl From<protocol::Doing> for Doing {
    fn from(doing: protocol::Doing) -> Self {
        match doing {
            protocol::Doing::Rest => Self::Unknown,
            protocol::Doing::Think => Self::Think,
            protocol::Doing::ReadFile => Self::Read(Paper::File),
            protocol::Doing::ReadMemory => Self::Read(Paper::Memory),
            protocol::Doing::ReadSkill => Self::Read(Paper::Skill),
            protocol::Doing::WriteFile => Self::Write(Paper::File),
            protocol::Doing::WriteMemory => {
                Self::Write(Paper::Memory)
            }
            protocol::Doing::WriteSkill => Self::Write(Paper::Skill),
            protocol::Doing::Run => Self::Run,
            protocol::Doing::Fetch => Self::Fetch,
            protocol::Doing::Custom => Self::CustomTool,
            protocol::Doing::Await => Self::Await,
        }
    }
}

/// An agent's current activity and the server's words for it.
#[derive(Component, Clone, Debug, Default, PartialEq, Eq)]
pub struct Activity {
    pub doing: Doing,
    pub words: String,
    pub step: u64,
    pub failures: u64,
}

/// The latest streamed words or reasoning of an agent.
#[derive(Component, Clone, Debug, Default, PartialEq, Eq)]
pub struct Saying {
    pub text: String,
    pub thought: bool,
}

/// An agent's model and received spend.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct Account {
    pub model: String,
    pub cost: String,
    pub spent: protocol::Amount,
}

/// The chief's name supplied by the server.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct Chief(pub String);

/// What confines the commands the server's agents run.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct Confinement(pub protocol::Confinement);

impl Default for Confinement {
    fn default() -> Self {
        Self(protocol::Confinement::Sandbox {
            name: String::new(),
        })
    }
}

/// The received model, network and reflection settings.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct Settings(pub protocol::SettingsView);

impl Default for Settings {
    fn default() -> Self {
        Self(protocol::SettingsView {
            model: String::new(),
            character: String::new(),
            effort: None,
            newcomers: protocol::Newcomers::default(),
            reflect_every: 0,
            allow_net: false,
            endpoints: Vec::new(),
            unset: true,
            link: protocol::Link {
                state: protocol::LinkState::Untested,
                models: None,
                words: None,
            },
            workspace: protocol::SettingsViewWorkspace {
                moving: None,
                running: None,
                url: None,
            },
        })
    }
}

/// The providers and login methods offered by the server.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct Providers(pub Vec<protocol::ProviderView>);

/// The lasting permissions supplied by the server.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct Grants(pub Vec<protocol::Grant>);

/// The names of the chief's deeds supported by the server.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct Deeds(pub Vec<String>);

impl Deeds {
    /// Membership in the server's supported deeds.
    pub fn is_supported(&self, name: &str) -> bool {
        self.0.iter().any(|deed| deed == name)
    }

    /// Whether the server supports the deed `name`, as `world` last
    /// heard: never before its state is received.
    pub fn is_allowed(world: &World, name: &str) -> bool {
        world
            .get_resource::<Deeds>()
            .is_some_and(|deeds| deeds.is_supported(name))
    }
}

/// The chief's deeds supported by the server, each with how its
/// command is typed and what it does.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct DeedUsage(pub Vec<protocol::Deed>);

/// The last applied server epoch and revision.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Seen {
    /// The server incarnation of the last applied frame.
    pub epoch: i64,
    /// The revision applied within this epoch.
    pub revision: i64,
}

/// The skill folder names in an agent's house.
#[derive(Component, Clone, Debug, Default, PartialEq, Eq)]
pub struct HouseSkills(pub Vec<String>);

/// The built-in tools a hire has turned off.
#[derive(Component, Clone, Debug, Default, PartialEq, Eq)]
pub struct ToolsOff(pub Vec<String>);

/// The library shelf, independent of agents' own skills.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct Skills(pub Vec<protocol::Skill>);

/// The removed hamlets available to restore or erase.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct Closed(pub Vec<protocol::ClosedHamlet>);

/// The received conversations with other agents.
#[derive(Component, Clone, Debug, Default, PartialEq)]
pub struct PeerChats(pub Vec<protocol::ConversationView>);

/// How and when an agent left, as the server sent it.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct Departure(pub protocol::Left);

#[derive(Component)]
struct Consulting;

/// Component registration for lookups before the first state arrives.
pub fn register(world: &mut World) {
    world.register_component::<HamletId>();
    world.register_component::<Name>();
    world.register_component::<Works>();
    world.register_component::<Bylaws>();
    world.register_component::<Treasury>();
    world.register_component::<Identity>();
    world.register_component::<Kind>();
    world.register_component::<Hire>();
    world.register_component::<Parent>();
    world.register_component::<OfProfile>();
    world.register_component::<Pin>();
    world.register_component::<Allowance>();
    world.register_component::<Nickname>();
    world.register_component::<Shown>();
    world.register_component::<OwnModel>();
    world.register_component::<OwnEffort>();
    world.register_component::<Fast>();
    world.register_component::<Profile>();
    world.register_component::<Purpose>();
    world.register_component::<Lived>();
    world.register_component::<LivesIn>();
    world.register_component::<Agents>();
    world.register_component::<Mayor>();
    world.register_component::<Busy>();
    world.register_component::<Faulted>();
    world.register_component::<Activity>();
    world.register_component::<Saying>();
    world.register_component::<Account>();
    world.register_component::<HouseSkills>();
    world.register_component::<ToolsOff>();
    world.register_component::<Left>();
    world.register_component::<MergedInto>();
    world.register_component::<Absorbed>();
    world.register_component::<Road>();
    world.register_component::<Traffic>();
    world.register_component::<Spot>();
    world.register_component::<PeerChats>();
}

/// The `Name` of an entity, or an empty string. An agent's is its
/// address, which commands name it by.
pub fn name_of(world: &World, entity: Entity) -> String {
    world
        .get::<Name>(entity)
        .map(|name| name.as_str().to_string())
        .unwrap_or_default()
}

/// What an entity is shown by: an agent's `Shown`, else its `Name`.
pub fn shown_of(world: &World, entity: Entity) -> String {
    world.get::<Shown>(entity).map_or_else(
        || name_of(world, entity),
        |shown| shown.0.clone(),
    )
}

/// The address commands name a profile by: `profile#<id>`.
pub fn profile_address(id: protocol::ProfileId) -> String {
    format!("profile#{}", id.0)
}

/// Lookups of living agents and mayors for systems.
#[derive(SystemParam)]
pub struct Roster<'w, 's> {
    index: Res<'w, Index>,
    hand: Option<Res<'w, HandId>>,
    mayors: Query<
        'w,
        's,
        (Entity, &'static Identity),
        (With<Mayor>, Without<Left>),
    >,
}

impl Roster<'_, '_> {
    /// A living agent by its address.
    pub fn named(&self, name: &str) -> Option<Entity> {
        self.index.named(name)
    }

    /// The Hand.
    pub fn hand(&self) -> Option<Entity> {
        self.index.agent(self.hand.as_ref()?.0)
    }

    /// The current mayor of the hamlet with the id `name`.
    pub fn mayor_of(&self, name: &str) -> Option<Entity> {
        self.index.mayor_of(name)
    }

    /// The living mayor that came first, whose hall is the oldest.
    pub fn first_mayor(&self) -> Option<Entity> {
        self.mayors
            .iter()
            .min_by_key(|(_, identity)| identity.0)
            .map(|(entity, _)| entity)
    }
}

/// A living agent by its address.
pub fn named(world: &World, name: &str) -> Option<Entity> {
    world.get_resource::<Index>()?.named(name)
}

/// The Hand.
pub fn hand(world: &World) -> Option<Entity> {
    let hand = world.get_resource::<HandId>()?.0;
    world.get_resource::<Index>()?.agent(hand)
}

/// The current mayor of the named hamlet.
pub fn mayor_of(world: &World, name: &str) -> Option<Entity> {
    world.get_resource::<Index>()?.mayor_of(name)
}

/// The living mayor that came first, whose hall is the oldest.
pub fn first_mayor(world: &World) -> Option<Entity> {
    let mut mayors = world
        .try_query_filtered::<(Entity, &Identity), (
            With<Mayor>,
            Without<Left>,
        )>()?;
    mayors
        .iter(world)
        .min_by_key(|(_, identity)| identity.0)
        .map(|(entity, _)| entity)
}

/// The protocol id of an agent's hamlet.
pub fn hamlet_of(world: &World, agent: Entity) -> Option<&str> {
    let lives = world.get::<LivesIn>(agent)?;
    Some(&world.get::<HamletId>(lives.0)?.0)
}

/// The id a message names a living agent by.
pub fn id_of(
    world: &World,
    agent: Entity,
) -> Option<protocol::AgentId> {
    if world.get::<Left>(agent).is_some() {
        return None;
    }
    Some(world.get::<Identity>(agent)?.0)
}

/// A hamlet by its protocol id.
pub fn hamlet(world: &World, id: &str) -> Option<Entity> {
    world.get_resource::<Index>()?.hamlet(id)
}

/// An agent by its id, living or not.
pub fn agent(world: &World, id: protocol::AgentId) -> Option<Entity> {
    world.get_resource::<Index>()?.agent(id)
}

/// A profile by its id.
pub fn profile(
    world: &World,
    id: protocol::ProfileId,
) -> Option<Entity> {
    world.get_resource::<Index>()?.profile(id)
}

/// An agent that has left its hamlet.
#[derive(Component, Clone, Copy, Debug)]
#[component(on_insert = Left::on_insert, on_discard = index::come_back)]
pub struct Left;

impl Left {
    fn on_insert(mut world: DeferredWorld, context: HookContext) {
        index::file_agent(world.reborrow(), context);
        // An agent that left keeps its entity, but none of its roads.
        let roads =
            roads_of(&world, context.entity).collect::<Vec<_>>();
        for road in roads {
            world.commands().entity(road).try_despawn();
        }
    }
}

/// The agent that absorbed this agent.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[relationship(relationship_target = Absorbed)]
pub struct MergedInto(pub Entity);

/// The agents merged into this agent.
#[derive(Component, Default, Debug)]
#[relationship_target(relationship = MergedInto)]
pub struct Absorbed(Vec<Entity>);

/// A road between two agent entities.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[component(immutable, on_insert = Road::on_insert)]
pub struct Road {
    pub ends: (Entity, Entity),
}

impl Road {
    /// Membership of an agent among the road's ends.
    pub fn has_end(&self, agent: Entity) -> bool {
        self.ends.0 == agent || self.ends.1 == agent
    }

    // Each end holds the road as a relationship, so the road goes
    // with either end.
    fn on_insert(mut world: DeferredWorld, context: HookContext) {
        let Some(&Road { ends: (from, to) }) =
            world.get::<Road>(context.entity)
        else {
            return;
        };
        world
            .commands()
            .entity(context.entity)
            .try_insert((RoadFrom(from), RoadTo(to)));
    }
}

/// A road's first end.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[relationship(relationship_target = RoadsFrom)]
pub struct RoadFrom(pub Entity);

/// The roads whose first end is this agent.
#[derive(Component, Default, Debug)]
#[relationship_target(relationship = RoadFrom, linked_spawn)]
pub struct RoadsFrom(Vec<Entity>);

/// A road's second end.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[relationship(relationship_target = RoadsTo)]
pub struct RoadTo(pub Entity);

/// The roads whose second end is this agent.
#[derive(Component, Default, Debug)]
#[relationship_target(relationship = RoadTo, linked_spawn)]
pub struct RoadsTo(Vec<Entity>);

/// The roads of an agent with these relationship targets.
pub fn roads<'a>(
    from: Option<&'a RoadsFrom>,
    to: Option<&'a RoadsTo>,
) -> impl Iterator<Item = Entity> + 'a {
    let from = from.into_iter().flat_map(RelationshipTarget::iter);
    from.chain(to.into_iter().flat_map(RelationshipTarget::iter))
}

/// The roads with an agent at one end.
pub fn roads_of(
    world: &World,
    agent: Entity,
) -> impl Iterator<Item = Entity> + '_ {
    roads(world.get(agent), world.get(agent))
}

/// The road between two agents.
pub fn road_between(
    world: &World,
    a: Entity,
    b: Entity,
) -> Option<Entity> {
    roads_of(world, a).find(|road| {
        world.get::<Road>(*road).is_some_and(|road| road.has_end(b))
    })
}

/// The drawn level of traffic on a road.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoadUse {
    Overgrown,
    Dirt,
    Paved,
    Rail,
}

/// A road's received traffic and latest sender.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Traffic {
    pub kind: protocol::RoadKind,
    pub level: RoadUse,
    pub messages: u64,
    pub last_from: Option<Entity>,
}

/// A letter carried along a road.
#[derive(Event, Clone, Copy, Debug)]
pub struct Carried {
    pub road: Entity,
    pub from: Entity,
}

/// An agent selected for consultation.
#[derive(Event, Clone, Copy, Debug)]
pub struct Consulted {
    pub agent: Option<Entity>,
}

/// A hamlet's optional place on the hamlet island, measured from its
/// middle.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct Spot(pub Option<(f32, f32)>);

#[cfg(test)]
mod tests;
