//! The hamlets and agents as the server last sent them.

use bevy_ecs::name::Name;
use bevy_ecs::prelude::*;
use bevy_ecs::system::{SystemParam, SystemState};
use hamlet_protocol::v0;

/// The server machine's houses folder, excluded from local chat
/// links.
#[derive(Resource, Default, Clone, Debug, PartialEq, Eq)]
pub struct Houses(pub Option<String>);

/// The first hamlet's protocol id.
pub const HOME: &str = "home";
/// The address of a hamlet's current mayor.
pub const MAYOR: &str = "mayor";
/// The received mode for edits and commands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Manual,
    Auto,
}

impl From<v0::HamletMode> for Mode {
    fn from(mode: v0::HamletMode) -> Self {
        match mode {
            v0::HamletMode::Manual => Self::Manual,
            v0::HamletMode::Auto => Self::Auto,
        }
    }
}

/// A hamlet identified by its protocol id.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct Settlement {
    /// The protocol id, distinct from the shown Name.
    pub name: String,
}

/// The hamlet's folder, earlier folders and mode words.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct Works {
    pub folder: Option<String>,
    pub before: Vec<String>,
    pub mode: Mode,
    pub words: String,
}

/// The chief's received limits for a hamlet.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct Bylaws(pub v0::Limits);

/// The server's ready-made budget report.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct Treasury(pub v0::Report);

/// An agent's lifelong number within its hamlet.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[require(Activity, Lived)]
pub struct Citizen(pub u64);

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
#[relationship(relationship_target = Citizens)]
pub struct LivesIn(pub Entity);

/// The agents belonging to a hamlet.
#[derive(Component, Default, Debug)]
#[relationship_target(relationship = LivesIn, linked_spawn)]
pub struct Citizens(Vec<Entity>);

/// The agent holding its hamlet's office.
#[derive(Component, Clone, Copy, Debug)]
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
    Notes,
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

impl From<v0::Doing> for Doing {
    fn from(doing: v0::Doing) -> Self {
        match doing {
            v0::Doing::Rest => Self::Unknown,
            v0::Doing::Think => Self::Think,
            v0::Doing::ReadFile => Self::Read(Paper::File),
            v0::Doing::ReadNotes => Self::Read(Paper::Notes),
            v0::Doing::ReadSkill => Self::Read(Paper::Skill),
            v0::Doing::WriteFile => Self::Write(Paper::File),
            v0::Doing::WriteNotes => Self::Write(Paper::Notes),
            v0::Doing::WriteSkill => Self::Write(Paper::Skill),
            v0::Doing::Run => Self::Run,
            v0::Doing::Fetch => Self::Fetch,
            v0::Doing::Custom => Self::CustomTool,
            v0::Doing::Await => Self::Await,
        }
    }
}

/// An agent's current activity and the server's words for it.
#[derive(Component, Clone, Debug, Default, PartialEq, Eq)]
pub struct Activity {
    pub doing: Doing,
    pub words: String,
    pub step: u32,
    pub failures: u32,
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
    pub spent: v0::Amount,
}

impl Account {
    /// The received model and cost words for an agent.
    pub fn summary(&self) -> String {
        if self.model.is_empty() {
            self.cost.clone()
        } else {
            format!("{}, {}", self.model, self.cost)
        }
    }
}

/// The chief's name supplied by the server.
#[derive(Resource, Clone, Debug, Default)]
pub struct Chief(pub String);

/// The server's sandbox name.
#[derive(Resource, Clone, Debug, Default)]
pub struct Sandbox(pub String);

/// The received model, network and reflection settings.
#[derive(Resource, Clone, Debug)]
pub struct Settings(pub v0::SettingsView);

impl Default for Settings {
    fn default() -> Self {
        Self(v0::SettingsView {
            model: String::new(),
            reflect_every: 0,
            allow_net: false,
            endpoints: Vec::new(),
            unset: true,
            link: v0::Link {
                state: v0::LinkState::Untested,
                models: None,
                words: None,
            },
        })
    }
}

/// The providers and login methods offered by the server.
#[derive(Resource, Clone, Debug, Default)]
pub struct Providers(pub Vec<v0::ProviderView>);

/// The lasting permissions supplied by the server.
#[derive(Resource, Clone, Debug, Default)]
pub struct Grants(pub Vec<v0::Grant>);

/// The chief's deeds supported by the server.
#[derive(Resource, Clone, Debug, Default)]
pub struct Deeds(pub Vec<String>);

impl Deeds {
    /// Membership in the server's supported deeds.
    pub fn has(&self, name: &str) -> bool {
        self.0.iter().any(|deed| deed == name)
    }
}

/// The last applied server epoch and revision.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Seen {
    /// The server incarnation of the last applied frame.
    pub epoch: i64,
    /// The revision applied within this epoch.
    pub revision: i64,
}

/// The shipped skills offered as gifts.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct Starters(pub Vec<v0::StateStarters>);

/// The skill folder names in an agent's house.
#[derive(Component, Clone, Debug, Default, PartialEq, Eq)]
pub struct HouseSkills(pub Vec<String>);

/// The library shelf, independent of agents' own skills.
#[derive(Resource, Clone, Debug, Default)]
pub struct Skills(pub Vec<v0::PapersSkills>);

/// The removed hamlets available to restore or erase.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct Closed(pub Vec<v0::ClosedHamlet>);

/// The votes and their ready-made reports for a hamlet.
#[derive(Component, Clone, Debug, Default, PartialEq)]
pub struct Votes(pub Vec<v0::VoteView>);

/// The received conversations with other agents.
#[derive(Component, Clone, Debug, Default, PartialEq)]
pub struct PeerChats(pub Vec<v0::ConversationView>);

#[derive(Component, Clone, Debug, PartialEq)]
struct Departure(v0::Left);

#[derive(Component)]
struct Consulting;

/// Component registration for lookups before the first state arrives.
pub fn register(world: &mut World) {
    world.register_component::<Settlement>();
    world.register_component::<Name>();
    world.register_component::<Works>();
    world.register_component::<Bylaws>();
    world.register_component::<Treasury>();
    world.register_component::<Citizen>();
    world.register_component::<Purpose>();
    world.register_component::<Lived>();
    world.register_component::<LivesIn>();
    world.register_component::<Citizens>();
    world.register_component::<Mayor>();
    world.register_component::<Busy>();
    world.register_component::<Faulted>();
    world.register_component::<Activity>();
    world.register_component::<Saying>();
    world.register_component::<Account>();
    world.register_component::<HouseSkills>();
    world.register_component::<Left>();
    world.register_component::<MergedInto>();
    world.register_component::<Absorbed>();
    world.register_component::<Road>();
    world.register_component::<Traffic>();
    world.register_component::<Spot>();
    world.register_component::<Votes>();
    world.register_component::<PeerChats>();
}

/// The shown name of an entity, or an empty string.
pub fn name_of(world: &World, entity: Entity) -> String {
    world
        .get::<Name>(entity)
        .map(|name| name.as_str().to_string())
        .unwrap_or_default()
}

/// Lookups of living agents and mayors for systems.
#[derive(SystemParam)]
pub struct Roster<'w, 's> {
    living: Query<
        'w,
        's,
        (Entity, &'static Name),
        (With<Citizen>, Without<Left>),
    >,
    mayors: Query<
        'w,
        's,
        (Entity, &'static LivesIn),
        (With<Mayor>, Without<Left>),
    >,
    settlements: Query<'w, 's, (Entity, &'static Settlement)>,
}

impl Roster<'_, '_> {
    /// A living agent by name, or the first hamlet's mayor by its
    /// office.
    pub fn named(&self, name: &str) -> Option<Entity> {
        if name == MAYOR {
            return self.mayor();
        }
        self.living
            .iter()
            .find(|(_, shown)| shown.as_str() == name)
            .map(|(entity, _)| entity)
    }

    /// The first hamlet's current mayor.
    pub fn mayor(&self) -> Option<Entity> {
        self.mayor_of(HOME)
    }

    /// The current mayor of the named hamlet.
    pub fn mayor_of(&self, name: &str) -> Option<Entity> {
        let hamlet = self
            .settlements
            .iter()
            .find(|(_, settlement)| settlement.name == name)
            .map(|(entity, _)| entity)?;
        self.mayors
            .iter()
            .find(|(_, lives)| lives.0 == hamlet)
            .map(|(entity, _)| entity)
    }
}

#[derive(Resource, Default)]
struct Asking(Option<SystemState<Roster<'static, 'static>>>);

fn ask<T>(
    world: &mut World,
    question: impl FnOnce(&Roster) -> T,
) -> Option<T> {
    let kept = world.get_resource_or_init::<Asking>().0.take();
    let mut state = kept.unwrap_or_else(|| SystemState::new(world));
    let answer =
        state.get(world).ok().map(|roster| question(&roster));
    world.resource_mut::<Asking>().0 = Some(state);
    answer
}

/// A living agent by name, or the first hamlet's mayor by its office.
pub fn named(world: &mut World, name: &str) -> Option<Entity> {
    ask(world, |roster| roster.named(name)).flatten()
}

/// The first hamlet's current mayor.
pub fn mayor(world: &mut World) -> Option<Entity> {
    mayor_of(world, HOME)
}

/// The current mayor of the named hamlet.
pub fn mayor_of(world: &mut World, name: &str) -> Option<Entity> {
    ask(world, |roster| roster.mayor_of(name)).flatten()
}

/// The protocol id of an agent's hamlet.
pub fn hamlet_of(world: &World, agent: Entity) -> Option<&str> {
    let lives = world.get::<LivesIn>(agent)?;
    Some(&world.get::<Settlement>(lives.0)?.name)
}

/// The wire address of a living agent.
pub fn address_of(
    world: &World,
    agent: Entity,
) -> Option<v0::Address> {
    world.get::<Citizen>(agent)?;
    if world.get::<Left>(agent).is_some() {
        return None;
    }
    Some(v0::Address {
        hamlet: hamlet_of(world, agent)?.into(),
        agent: world.get::<Name>(agent)?.as_str().into(),
    })
}

fn settlement(world: &mut World, id: &str) -> Option<Entity> {
    world
        .query::<(Entity, &Settlement)>()
        .iter(world)
        .find(|(_, settlement)| settlement.name == id)
        .map(|(entity, _)| entity)
}

/// An agent that has left its hamlet.
#[derive(Component, Clone, Copy, Debug)]
pub struct Left;

pub(crate) fn state(world: &mut World, state: v0::State) {
    let news = world.contains_resource::<Seen>();
    world.insert_resource(Houses(state.houses));
    world.insert_resource(Skills(
        state
            .library
            .unwrap_or_default()
            .into_iter()
            .map(|skill| v0::PapersSkills {
                name: skill.name,
                description: skill.description,
            })
            .collect(),
    ));
    world.insert_resource(Closed(state.closed.unwrap_or_default()));
    world.insert_resource(Chief(state.chief));
    world.insert_resource(Sandbox(state.sandbox));
    world.insert_resource(Settings(state.settings));
    world.insert_resource(Providers(state.providers));
    world.insert_resource(Grants(state.grants));
    world.insert_resource(Deeds(state.deeds.unwrap_or_default()));
    world.insert_resource(Starters(
        state.starters.unwrap_or_default(),
    ));
    world.insert_resource(Seen {
        epoch: state.epoch,
        revision: state.revision,
    });
    let hamlet_values = state.hamlets;
    let hamlets = hamlet_values
        .iter()
        .cloned()
        .map(|hamlet| upsert_hamlet(world, hamlet))
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
    let gone = world
        .query_filtered::<Entity, With<crate::waits::Waiting>>()
        .iter(world)
        .filter(|entity| !waits.contains(entity))
        .collect::<Vec<_>>();
    for entity in gone {
        world.despawn(entity);
    }
    let gone = world
        .query_filtered::<Entity, With<Citizen>>()
        .iter(world)
        .filter(|entity| !agents.contains(entity))
        .collect::<Vec<_>>();
    for entity in gone {
        world.despawn(entity);
    }
    let gone = world
        .query_filtered::<Entity, With<Settlement>>()
        .iter(world)
        .filter(|entity| !hamlets.contains(entity))
        .collect::<Vec<_>>();
    for entity in gone {
        world.despawn(entity);
    }
    prune_roads(world);
}

fn upsert_hamlet(world: &mut World, hamlet: v0::Hamlet) -> Entity {
    let entity = settlement(world, &hamlet.id)
        .unwrap_or_else(|| world.spawn_empty().id());
    let mut entity_mut = world.entity_mut(entity);
    put(&mut entity_mut, Settlement { name: hamlet.id });
    put(&mut entity_mut, Name::new(hamlet.name));
    put(
        &mut entity_mut,
        Works {
            folder: hamlet.folder,
            before: hamlet.before,
            mode: hamlet.mode.into(),
            words: hamlet.mode_words,
        },
    );
    put(&mut entity_mut, Bylaws(hamlet.limits));
    put(&mut entity_mut, Treasury(hamlet.budget));
    put(
        &mut entity_mut,
        Spot(
            hamlet.at.flatten().map(|at| (at.x as f32, at.z as f32)),
        ),
    );
    entity
}

pub(crate) fn agent(
    world: &mut World,
    reference: &v0::AgentRef,
) -> Option<Entity> {
    world
        .query::<(Entity, &Citizen, &LivesIn)>()
        .iter(world)
        .find(|(_, citizen, lives)| {
            citizen.0 == reference.number as u64
                && world.get::<Settlement>(lives.0).is_some_and(
                    |hamlet| hamlet.name == reference.hamlet,
                )
        })
        .map(|(entity, _, _)| entity)
}

fn upsert_agent(
    world: &mut World,
    value: v0::Agent,
) -> Option<Entity> {
    let hamlet = settlement(world, &value.ref_.hamlet)?;
    let entity = agent(world, &value.ref_)
        .unwrap_or_else(|| world.spawn_empty().id());
    let consulting = value.consulting.unwrap_or(false);
    let consulted =
        consulting && world.get::<Consulting>(entity).is_none();
    let mut agent = world.entity_mut(entity);
    put(&mut agent, Citizen(value.ref_.number as u64));
    put(&mut agent, Name::new(value.name));
    put(&mut agent, Purpose(value.purpose));
    put(&mut agent, HouseSkills(value.skills.unwrap_or_default()));
    put(
        &mut agent,
        Lived {
            since: value.since as u64,
            until: value.left.as_ref().map(|left| left.at as u64),
        },
    );
    put(&mut agent, LivesIn(hamlet));
    put(
        &mut agent,
        Account {
            model: value.model,
            cost: value.spent.cost,
            spent: v0::Amount {
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
            step: value.activity.step as u32,
            failures: value.activity.failures as u32,
        },
    );
    put(
        &mut agent,
        Saying {
            text: value.saying.text,
            thought: value.saying.thought,
        },
    );
    flag(&mut agent, value.mayor, Mayor);
    flag(&mut agent, value.busy, Busy);
    flag(&mut agent, value.faulted, Faulted);

    flag(&mut agent, consulting, Consulting);
    put(
        &mut agent,
        PeerChats(value.conversations.unwrap_or_default()),
    );
    if let Some(left) = value.left {
        put(&mut agent, Departure(left));
    } else {
        agent.remove::<(Left, Departure, MergedInto)>();
    }
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

pub(crate) fn changes(world: &mut World, changes: v0::Changes) {
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
            v0::Update::Hamlet { hamlet } => {
                upsert_hamlet(world, hamlet.clone());
                hamlets.push(hamlet);
            }
            v0::Update::Agent { agent } => {
                upsert_agent(world, agent);
            }
            v0::Update::Settings { settings } => {
                world.insert_resource(Settings(settings));
            }
            v0::Update::Providers { providers } => {
                world.insert_resource(Providers(providers));
            }
            v0::Update::Grants { grants } => {
                world.insert_resource(Grants(grants));
            }
            v0::Update::Wait { wait } => {
                crate::waits::upsert(world, wait);
            }
            v0::Update::WaitClosed { id, .. } => {
                if let Some(entity) = crate::waits::find(world, &id) {
                    world.despawn(entity);
                }
            }
            v0::Update::AgentGone { ref_ } => {
                if let Some(entity) = agent(world, &ref_) {
                    world.despawn(entity);
                }
            }
        }
    }
    departures(world);
    for hamlet in hamlets {
        society(world, hamlet, true);
    }
    prune_roads(world);
    world.insert_resource(Seen {
        epoch: changes.epoch,
        revision: changes.revision,
    });
}

fn departures(world: &mut World) {
    let departed = world
        .query::<(Entity, &Departure)>()
        .iter(world)
        .map(|(entity, left)| (entity, left.0.into.clone()))
        .collect::<Vec<_>>();
    for (entity, into) in departed {
        let host = into.as_ref().and_then(|into| agent(world, into));
        let mut entity = world.entity_mut(entity);
        if let Some(host) = host {
            entity.insert((MergedInto(host), Left));
        } else {
            entity.remove::<MergedInto>();
            entity.insert(Left);
        }
    }
}

fn society(world: &mut World, value: v0::Hamlet, news: bool) {
    let Some(hamlet) = settlement(world, &value.id) else {
        return;
    };
    let votes = value.votes.unwrap_or_default();
    let was = world.get::<Votes>(hamlet);
    let cast = votes
        .iter()
        .flat_map(|vote| {
            let before = was.and_then(|was| {
                was.0.iter().find(|v| v.id == vote.id)
            });
            vote.voters
                .iter()
                .filter(move |voter| {
                    news && voter.cast.is_some()
                        && before
                            .and_then(|v| {
                                v.voters
                                    .iter()
                                    .find(|v| v.agent == voter.agent)
                            })
                            .is_none_or(|v| v.cast.is_none())
                })
                .map(|voter| voter.agent.clone())
        })
        .collect::<Vec<_>>();
    put(&mut world.entity_mut(hamlet), Votes(votes));
    for voter in cast {
        if let Some(agent) = agent(world, &voter) {
            world.trigger(Voted { agent });
        }
    }
    let mut wanted = Vec::new();
    for value in value.roads.unwrap_or_default() {
        let (Some(a), Some(b)) =
            (agent(world, &value.a), agent(world, &value.b))
        else {
            continue;
        };
        let ends = if a <= b { (a, b) } else { (b, a) };
        let road = world
            .query::<(Entity, &Road)>()
            .iter(world)
            .find(|(_, road)| road.ends == ends)
            .map(|(e, _)| e);
        let before = road
            .and_then(|e| world.get::<Traffic>(e))
            .map_or(0, |t| t.messages);
        let traffic = Traffic {
            level: match value.level {
                v0::RoadLevel::Overgrown => RoadUse::Overgrown,
                v0::RoadLevel::Dirt => RoadUse::Dirt,
                v0::RoadLevel::Paved => RoadUse::Paved,
                v0::RoadLevel::Rail => RoadUse::Rail,
            },
            messages: value.messages as u32,
            last_from: value
                .last_from
                .as_ref()
                .and_then(|from| agent(world, from)),
        };
        let entity =
            road.unwrap_or_else(|| world.spawn(Road { ends }).id());
        put(&mut world.entity_mut(entity), traffic);
        wanted.push(entity);
        if news
            && traffic.messages > before
            && let Some(from) = traffic.last_from
        {
            world.trigger(Carried { road: entity, from });
        }
    }
    let gone = world
        .query::<(Entity, &Road)>()
        .iter(world)
        .filter(|(entity, road)| {
            !wanted.contains(entity)
                && [road.ends.0, road.ends.1].iter().any(|end| {
                    world
                        .get::<LivesIn>(*end)
                        .is_some_and(|l| l.0 == hamlet)
                })
        })
        .map(|(e, _)| e)
        .collect::<Vec<_>>();
    for entity in gone {
        world.despawn(entity);
    }
}

fn prune_roads(world: &mut World) {
    let gone = world
        .query::<(Entity, &Road)>()
        .iter(world)
        .filter(|(_, road)| {
            [road.ends.0, road.ends.1].iter().any(|end| {
                world.get::<Citizen>(*end).is_none()
                    || world.get::<Left>(*end).is_some()
            })
        })
        .map(|(e, _)| e)
        .collect::<Vec<_>>();
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

/// A road between two agent entities.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Road {
    pub ends: (Entity, Entity),
}

impl Road {
    /// Membership of an agent among the road's ends.
    pub fn has(&self, agent: Entity) -> bool {
        self.ends.0 == agent || self.ends.1 == agent
    }
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
    pub level: RoadUse,
    pub messages: u32,
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

/// An agent participating in a vote.
#[derive(Event, Clone, Copy, Debug)]
pub struct Voted {
    pub agent: Entity,
}

/// A town's optional place in the world.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct Spot(pub Option<(f32, f32)>);
