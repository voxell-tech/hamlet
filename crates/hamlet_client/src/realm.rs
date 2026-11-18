//! The hamlets and agents as the server last sent them.

use bevy_ecs::component::Mutable;
use bevy_ecs::lifecycle::HookContext;
use bevy_ecs::name::Name;
use bevy_ecs::prelude::*;
use bevy_ecs::system::SystemParam;
use bevy_ecs::world::DeferredWorld;
use hamlet_protocol::client as protocol;
pub use hamlet_protocol::{HAND, LEAD};

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
            channels_between: true,
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
    /// The most agents it holds.
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

/// Whether an agent is the Hand, an agent of a hamlet or a worker.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Kind(pub protocol::AgentKind);

impl Default for Kind {
    fn default() -> Self {
        Self(protocol::AgentKind::Hamlet)
    }
}

/// An agent of a hamlet, for queries that want only those.
#[derive(Component, Clone, Copy, Debug)]
pub struct HamletAgent;

/// The agent an agent of a hamlet or a worker answers to.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Parent(pub protocol::AgentId);

/// The profile an agent is of.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct OfProfile(pub protocol::ProfileId);

/// What an agent is called in the world, while it has a name of its
/// own.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct AgentName(pub String);

/// What an agent is shown by: its name, else its profile's name, else
/// its address.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct Shown(pub String);

/// The version of its profile an agent is pinned to.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct Pin(pub String);

/// The dollars an agent may spend in its hamlet's budget period.
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
pub struct Lead;

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

/// The built-in tools an agent has turned off.
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
    world.register_component::<HamletAgent>();
    world.register_component::<Parent>();
    world.register_component::<OfProfile>();
    world.register_component::<Pin>();
    world.register_component::<Allowance>();
    world.register_component::<AgentName>();
    world.register_component::<Shown>();
    world.register_component::<OwnModel>();
    world.register_component::<OwnEffort>();
    world.register_component::<Fast>();
    world.register_component::<Profile>();
    world.register_component::<Purpose>();
    world.register_component::<Lived>();
    world.register_component::<LivesIn>();
    world.register_component::<Agents>();
    world.register_component::<Lead>();
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
    world.register_component::<Channel>();
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

/// Lookups of living agents and leads for systems.
#[derive(SystemParam)]
pub struct Roster<'w, 's> {
    index: Res<'w, Index>,
    hand: Option<Res<'w, HandId>>,
    leads: Query<
        'w,
        's,
        (Entity, &'static Identity),
        (With<Lead>, Without<Left>),
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

    /// The current lead of the hamlet with the id `name`.
    pub fn lead_of(&self, name: &str) -> Option<Entity> {
        self.index.lead_of(name)
    }

    /// The living lead that came first, whose hall is the oldest.
    pub fn first_lead(&self) -> Option<Entity> {
        self.leads
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

/// The current lead of the named hamlet.
pub fn lead_of(world: &World, name: &str) -> Option<Entity> {
    world.get_resource::<Index>()?.lead_of(name)
}

/// The living lead that came first, whose hall is the oldest.
pub fn first_lead(world: &World) -> Option<Entity> {
    let mut leads = world
        .try_query_filtered::<(Entity, &Identity), (
            With<Lead>,
            Without<Left>,
        )>()?;
    leads
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
        // An agent that left keeps its entity, but none of its
        // channels.
        let channels =
            channels_of(&world, context.entity).collect::<Vec<_>>();
        for channel in channels {
            world.commands().entity(channel).try_despawn();
        }
    }
}

/// Inserts `value` unless the held resource already equals it.
fn replace<R: Resource<Mutability = Mutable> + PartialEq>(
    world: &mut World,
    value: R,
) {
    match world.get_resource_mut::<R>() {
        Some(mut held) => {
            held.set_if_neq(value);
        }
        None => world.insert_resource(value),
    }
}

pub(crate) fn state(world: &mut World, state: protocol::State) {
    let news = world.contains_resource::<Seen>();
    // An insert marks a resource changed even with the same value,
    // and a full state resends everything.
    replace(world, RealmFolder(state.realm));
    replace(world, HandId(state.hand));
    replace(world, RealmBylaws(state.limits));
    replace(
        world,
        Skills(
            state
                .library
                .into_iter()
                .map(|skill| protocol::Skill {
                    name: skill.name,
                    description: skill.description,
                })
                .collect(),
        ),
    );
    replace(world, Closed(state.closed));
    replace(world, Chief(state.chief));
    replace(world, Confinement(state.confinement));
    replace(world, Settings(state.settings));
    replace(world, Providers(state.providers));
    replace(world, Grants(state.grants));
    replace(
        world,
        Deeds(
            state
                .deeds
                .iter()
                .map(|deed| deed.name.clone())
                .collect(),
        ),
    );
    replace(world, DeedUsage(state.deeds));
    world.insert_resource(Seen {
        epoch: state.epoch,
        revision: state.revision,
    });
    let terminals = state.terminals;
    let hamlet_values = state.hamlets;
    let hamlets = hamlet_values
        .iter()
        .cloned()
        .map(|hamlet| upsert_hamlet(world, hamlet))
        .collect::<Vec<_>>();
    let profiles = state
        .profiles
        .into_iter()
        .map(|profile| upsert_profile(world, profile))
        .collect::<Vec<_>>();
    let agents = state
        .agents
        .into_iter()
        .filter_map(|agent| upsert_agent(world, agent))
        .collect::<Vec<_>>();
    departures(world);
    for hamlet in hamlet_values {
        society(world, hamlet, news);
    }
    let waits = state
        .waits
        .into_iter()
        .map(|wait| crate::waits::upsert(world, wait))
        .collect::<Vec<_>>();
    let index = world.resource::<Index>();
    let gone = index
        .waits()
        .filter(|entity| !waits.contains(entity))
        .chain(
            index.agents().filter(|entity| !agents.contains(entity)),
        )
        .chain(
            index
                .profiles()
                .filter(|entity| !profiles.contains(entity)),
        )
        .chain(
            index
                .hamlets()
                .filter(|entity| !hamlets.contains(entity)),
        )
        .collect::<Vec<_>>();
    for entity in gone {
        // A hamlet's agents go with it, so one may be gone already.
        world.try_despawn(entity).ok();
    }
    crate::terminals::state(world, terminals);
}

fn upsert_hamlet(
    world: &mut World,
    value: protocol::Hamlet,
) -> Entity {
    let entity = hamlet(world, &value.id)
        .unwrap_or_else(|| world.spawn_empty().id());
    let mut entity_mut = world.entity_mut(entity);
    put(&mut entity_mut, HamletId(value.id));
    put(&mut entity_mut, Name::new(value.name));
    put(
        &mut entity_mut,
        Works {
            folder: value.folder,
            before: value.before,
            mode: value.mode.into(),
            words: value.mode_words,
            tier: value.tier,
        },
    );
    put(&mut entity_mut, Bylaws(value.limits));
    put(&mut entity_mut, Treasury(value.budget));
    put(
        &mut entity_mut,
        Spot(value.at.map(|at| (at.x as f32, at.z as f32))),
    );
    entity
}

fn upsert_profile(
    world: &mut World,
    value: protocol::Profile,
) -> Entity {
    let entity = profile(world, value.id)
        .unwrap_or_else(|| world.spawn_empty().id());
    let id = value.id;
    let mut entity_mut = world.entity_mut(entity);
    put(&mut entity_mut, Name::new(value.name.clone()));
    put(&mut entity_mut, Profile(value));
    // A rename changes what its agents are shown by.
    let agents = world
        .query::<(Entity, &OfProfile)>()
        .iter(world)
        .filter(|(_, of)| of.0 == id)
        .map(|(agent, _)| agent)
        .collect::<Vec<_>>();
    for agent in agents {
        refresh_shown(world, agent);
    }
    entity
}

/// Sets what the agent is shown by from its name, its profile's name
/// and its address.
fn refresh_shown(world: &mut World, agent: Entity) {
    let Some(address) = world.get::<Name>(agent) else {
        return;
    };
    let shown = world
        .get::<AgentName>(agent)
        .map(|name| name.0.clone())
        .or_else(|| {
            let of = world.get::<OfProfile>(agent)?;
            let profile =
                world.get::<Profile>(profile(world, of.0)?)?;
            Some(profile.0.name.clone())
        })
        .unwrap_or_else(|| address.as_str().to_owned());
    put(&mut world.entity_mut(agent), Shown(shown));
}

fn upsert_agent(
    world: &mut World,
    value: protocol::Agent,
) -> Option<Entity> {
    // An agent whose hamlet has not arrived waits for its next frame.
    let hamlet = match &value.hamlet {
        Some(id) => Some(hamlet(world, id)?),
        None => None,
    };
    let entity = agent(world, value.id)
        .unwrap_or_else(|| world.spawn_empty().id());
    let consulting = value.consulting;
    let consulted =
        consulting && world.get::<Consulting>(entity).is_none();
    let mut agent = world.entity_mut(entity);
    put(&mut agent, Identity(value.id));
    put(&mut agent, Kind(value.kind));
    put(&mut agent, Name::new(value.address));
    flag(
        &mut agent,
        value.kind == protocol::AgentKind::Hamlet,
        HamletAgent,
    );
    optional(&mut agent, value.parent.map(Parent));
    optional(&mut agent, value.profile.map(OfProfile));
    optional(&mut agent, value.pin.clone().map(Pin));
    optional(&mut agent, value.allowance.map(Allowance));
    optional(&mut agent, value.name.clone().map(AgentName));
    optional(&mut agent, value.own_model.clone().map(OwnModel));
    optional(&mut agent, value.effort.map(OwnEffort));
    flag(&mut agent, value.fast, Fast);
    put(&mut agent, Purpose(value.purpose));
    put(&mut agent, HouseSkills(value.skills));
    put(&mut agent, ToolsOff(value.tools_off));
    put(
        &mut agent,
        Lived {
            since: value.since,
            until: value.left.as_ref().map(|left| left.at),
        },
    );
    optional(&mut agent, hamlet.map(LivesIn));
    put(
        &mut agent,
        Account {
            model: value.model,
            cost: value.spent.cost,
            spent: protocol::Amount {
                tokens: value.spent.tokens,
                micros: value.spent.micros,
                ms: value.spent.ms,
            },
        },
    );
    put(
        &mut agent,
        Activity {
            doing: value.activity.doing.into(),
            words: value.activity.doing_words,
            step: value.activity.step,
            failures: value.activity.failures,
        },
    );
    put(
        &mut agent,
        Saying {
            text: value.saying.text,
            thought: value.saying.thought,
        },
    );
    flag(&mut agent, value.lead, Lead);
    flag(&mut agent, value.busy, Busy);
    flag(&mut agent, value.faulted, Faulted);

    flag(&mut agent, consulting, Consulting);
    put(&mut agent, PeerChats(value.conversations));
    if let Some(left) = value.left {
        put(&mut agent, Departure(left));
    } else {
        agent.remove::<(Left, Departure, MergedInto)>();
    }
    refresh_shown(world, entity);
    if consulted {
        world.trigger(Consulted {
            agent: Some(entity),
        });
    }
    Some(entity)
}

fn put<T: Component + PartialEq>(
    entity: &mut EntityWorldMut,
    value: T,
) {
    if entity.get::<T>() != Some(&value) {
        entity.insert(value);
    }
}

/// Puts `value`, or removes the component when there is none.
fn optional<T: Component + PartialEq>(
    entity: &mut EntityWorldMut,
    value: Option<T>,
) {
    match value {
        Some(value) => put(entity, value),
        None => {
            entity.remove::<T>();
        }
    }
}

fn flag<T: Component>(
    entity: &mut EntityWorldMut,
    present: bool,
    value: T,
) {
    if present && !entity.contains::<T>() {
        entity.insert(value);
    } else if !present && entity.contains::<T>() {
        entity.remove::<T>();
    }
}

pub(crate) fn changes(world: &mut World, changes: protocol::Changes) {
    let Some(seen) = world.get_resource::<Seen>() else {
        return;
    };
    if changes.epoch != seen.epoch
        || changes.revision <= seen.revision
    {
        return;
    }
    let mut hamlets = Vec::new();
    for update in changes.changes {
        match update {
            protocol::Update::Hamlet { hamlet } => {
                upsert_hamlet(world, hamlet.clone());
                hamlets.push(hamlet);
            }
            protocol::Update::Agent { agent } => {
                upsert_agent(world, agent);
            }
            protocol::Update::Settings { settings } => {
                world.insert_resource(Settings(settings));
            }
            protocol::Update::Providers { providers } => {
                world.insert_resource(Providers(providers));
            }
            protocol::Update::Grants { grants } => {
                world.insert_resource(Grants(grants));
            }
            protocol::Update::Wait { wait } => {
                crate::waits::upsert(world, wait);
            }
            protocol::Update::WaitClosed { id, .. } => {
                if let Some(entity) = crate::waits::find(world, &id) {
                    world.despawn(entity);
                }
            }
            protocol::Update::AgentGone { id } => {
                if let Some(entity) = agent(world, id) {
                    world.despawn(entity);
                }
            }
            protocol::Update::Profile { profile } => {
                upsert_profile(world, profile);
            }
            protocol::Update::ProfileGone { id } => {
                if let Some(entity) = profile(world, id) {
                    world.despawn(entity);
                }
            }
            protocol::Update::Terminal { terminal } => {
                crate::terminals::upsert(world, terminal);
            }
            protocol::Update::TerminalGone { id } => {
                crate::terminals::gone(world, &id);
            }
        }
    }
    departures(world);
    for hamlet in hamlets {
        society(world, hamlet, true);
    }
    world.insert_resource(Seen {
        epoch: changes.epoch,
        revision: changes.revision,
    });
}

fn departures(world: &mut World) {
    let departed = world
        .resource::<Index>()
        .agents()
        .filter_map(|entity| {
            let left = world.get::<Departure>(entity)?;
            Some((entity, left.0.into))
        })
        .collect::<Vec<_>>();
    for (entity, into) in departed {
        let host = into.and_then(|into| agent(world, into));
        let mut entity = world.entity_mut(entity);
        // Only on a difference: `Left` inserted again would file the
        // agent and look for its channels once more.
        let merged =
            entity.get::<MergedInto>().map(|merged| merged.0);
        if entity.contains::<Left>() && merged == host {
            continue;
        }
        if let Some(host) = host {
            entity.insert((MergedInto(host), Left));
        } else {
            entity.remove::<MergedInto>();
            entity.insert(Left);
        }
    }
}

fn society(world: &mut World, value: protocol::Hamlet, news: bool) {
    let Some(hamlet) = hamlet(world, &value.id) else {
        return;
    };
    let mut wanted = Vec::new();
    for value in value.channels {
        let (Some(a), Some(b)) =
            (agent(world, value.a), agent(world, value.b))
        else {
            continue;
        };
        // An agent that left has no channels, though its hamlet still
        // lists them.
        if world.get::<Left>(a).is_some()
            || world.get::<Left>(b).is_some()
        {
            continue;
        }
        let ends = if a <= b { (a, b) } else { (b, a) };
        let channel = channel_between(world, a, b);
        let before = channel
            .and_then(|e| world.get::<Traffic>(e))
            .map_or(0, |t| t.messages);
        let traffic = Traffic {
            level: match value.level {
                protocol::ChannelLevel::Overgrown => {
                    ChannelUse::Overgrown
                }
                protocol::ChannelLevel::Dirt => ChannelUse::Dirt,
                protocol::ChannelLevel::Paved => ChannelUse::Paved,
                protocol::ChannelLevel::Rail => ChannelUse::Rail,
            },
            kind: value.kind,
            messages: value.messages,
            last_from: value
                .last_from
                .and_then(|from| agent(world, from)),
        };
        let entity = channel
            .unwrap_or_else(|| world.spawn(Channel { ends }).id());
        put(&mut world.entity_mut(entity), traffic);
        wanted.push(entity);
        if news
            && traffic.messages > before
            && let Some(from) = traffic.last_from
        {
            world.trigger(Carried {
                channel: entity,
                from,
            });
        }
    }
    let mut gone = world
        .get::<Agents>(hamlet)
        .into_iter()
        .flat_map(RelationshipTarget::iter)
        .flat_map(|agent| channels_of(world, agent))
        .filter(|channel| !wanted.contains(channel))
        .collect::<Vec<_>>();
    // A channel between two of the hamlet's agents is met at each
    // end.
    gone.sort_unstable();
    gone.dedup();
    for entity in gone {
        world.despawn(entity);
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

/// A channel between two agent entities.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[component(immutable, on_insert = Channel::on_insert)]
pub struct Channel {
    pub ends: (Entity, Entity),
}

impl Channel {
    /// Membership of an agent among the channel's ends.
    pub fn has_end(&self, agent: Entity) -> bool {
        self.ends.0 == agent || self.ends.1 == agent
    }

    // Each end holds the channel as a relationship, so the channel
    // goes with either end.
    fn on_insert(mut world: DeferredWorld, context: HookContext) {
        let Some(&Channel { ends: (from, to) }) =
            world.get::<Channel>(context.entity)
        else {
            return;
        };
        world
            .commands()
            .entity(context.entity)
            .try_insert((ChannelFrom(from), ChannelTo(to)));
    }
}

/// A channel's first end.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[relationship(relationship_target = ChannelsFrom)]
pub struct ChannelFrom(pub Entity);

/// The channels whose first end is this agent.
#[derive(Component, Default, Debug)]
#[relationship_target(relationship = ChannelFrom, linked_spawn)]
pub struct ChannelsFrom(Vec<Entity>);

/// A channel's second end.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[relationship(relationship_target = ChannelsTo)]
pub struct ChannelTo(pub Entity);

/// The channels whose second end is this agent.
#[derive(Component, Default, Debug)]
#[relationship_target(relationship = ChannelTo, linked_spawn)]
pub struct ChannelsTo(Vec<Entity>);

/// The channels of an agent with these relationship targets.
pub fn channels<'a>(
    from: Option<&'a ChannelsFrom>,
    to: Option<&'a ChannelsTo>,
) -> impl Iterator<Item = Entity> + 'a {
    let from = from.into_iter().flat_map(RelationshipTarget::iter);
    from.chain(to.into_iter().flat_map(RelationshipTarget::iter))
}

/// The channels with an agent at one end.
pub fn channels_of(
    world: &World,
    agent: Entity,
) -> impl Iterator<Item = Entity> + '_ {
    channels(world.get(agent), world.get(agent))
}

/// The channel between two agents.
pub fn channel_between(
    world: &World,
    a: Entity,
    b: Entity,
) -> Option<Entity> {
    channels_of(world, a).find(|channel| {
        world
            .get::<Channel>(*channel)
            .is_some_and(|channel| channel.has_end(b))
    })
}

/// The drawn level of traffic on a channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelUse {
    Overgrown,
    Dirt,
    Paved,
    Rail,
}

/// A channel's received traffic and latest sender.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Traffic {
    pub kind: protocol::ChannelKind,
    pub level: ChannelUse,
    pub messages: u64,
    pub last_from: Option<Entity>,
}

/// A letter carried along a channel.
#[derive(Event, Clone, Copy, Debug)]
pub struct Carried {
    pub channel: Entity,
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
mod tests {
    use bevy_app::App;
    use hamlet_protocol::client as protocol;
    use hamlet_testing::realm::{agent, channel, state};

    use super::{channel_between, channels_of, named, shown_of};
    use crate::{ClientPlugin, receive};

    #[test]
    fn an_agent_is_shown_by_its_name_else_its_profiles_name() {
        let mut app = App::new();
        app.add_plugins(ClientPlugin { server: None });
        let mut realm = state();
        let world = app.world_mut();
        receive(world, protocol::ServerMessage::State(realm.clone()));
        let ada = named(world, "agent#2").unwrap();
        assert_eq!(shown_of(world, ada), "ada");
        realm.revision = 1;
        realm.profiles[0].name = "Ada Lovelace".into();
        receive(world, protocol::ServerMessage::State(realm.clone()));
        assert_eq!(shown_of(world, ada), "Ada Lovelace");
        realm.revision = 2;
        realm.agents[1].name = Some("Countess".into());
        receive(world, protocol::ServerMessage::State(realm));
        assert_eq!(shown_of(world, ada), "Countess");
        assert_eq!(named(world, "agent#2"), Some(ada));
    }

    #[test]
    fn a_channel_goes_with_an_end_that_left_or_is_gone_and_is_not_opened_again()
     {
        let mut app = App::new();
        app.add_plugins(ClientPlugin { server: None });
        let mut realm = state();
        realm.agents.push(agent(3, "home", false));
        realm.agents.push(agent(4, "home", false));
        let ada = protocol::AgentId(2);
        realm.hamlets[0].channels = vec![
            channel(ada, protocol::AgentId(3), 0),
            channel(ada, protocol::AgentId(4), 0),
        ];
        let world = app.world_mut();
        receive(world, protocol::ServerMessage::State(realm.clone()));
        let [ada, scout, miner] = ["agent#2", "agent#3", "agent#4"]
            .map(|name| named(world, name).unwrap());
        assert_eq!(channels_of(world, ada).count(), 2);
        realm.agents[2].left = Some(protocol::Left {
            at: 1,
            by: "chief".into(),
            how: protocol::LeftHow::Ended,
            into: None,
            reason: String::new(),
            words: String::new(),
        });
        realm.revision += 1;
        receive(world, protocol::ServerMessage::State(realm.clone()));
        assert!(world.get_entity(scout).is_ok());
        assert_eq!(channel_between(world, ada, scout), None);
        let mine = channel_between(world, ada, miner).unwrap();
        realm.agents.pop();
        realm.revision += 1;
        receive(world, protocol::ServerMessage::State(realm));
        assert!(world.get_entity(mine).is_err());
        assert_eq!(channels_of(world, ada).count(), 0);
    }
}
