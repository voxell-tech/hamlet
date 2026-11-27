//! What the server sends of the realm, put into the world.

use bevy_ecs::component::Mutable;
use bevy_ecs::name::Name;
use bevy_ecs::prelude::*;
use hamlet_protocol::client as protocol;

use super::{
    Account, Activity, Agents, Allowance, Busy, Bylaws, Carried,
    Chief, Closed, Confinement, Consulted, Consulting, DeedUsage,
    Deeds, Departure, Fast, Faulted, Grants, HamletId, HandId, Hire,
    HouseSkills, Identity, Kind, Left, Lived, LivesIn, Mayor,
    MergedInto, Nickname, OfProfile, OwnEffort, OwnModel, Parent,
    PeerChats, Pin, Profile, Providers, Purpose, RealmBylaws,
    RealmFolder, Road, RoadUse, Saying, Seen, Settings, Shown,
    Skills, Spot, ToolsOff, Traffic, Treasury, Works, agent, hamlet,
    profile, road_between, roads_of,
};
use crate::index::Index;

/// Inserts `value` unless the held resource already equals it.
pub(super) fn replace<
    R: Resource<Mutability = Mutable> + PartialEq,
>(
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
}

pub(super) fn upsert_hamlet(
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

pub(super) fn upsert_profile(
    world: &mut World,
    value: protocol::Profile,
) -> Entity {
    let entity = profile(world, value.id)
        .unwrap_or_else(|| world.spawn_empty().id());
    let id = value.id;
    let mut entity_mut = world.entity_mut(entity);
    put(&mut entity_mut, Name::new(value.name.clone()));
    put(&mut entity_mut, Profile(value));
    // A rename changes what its hires are shown by.
    let hires = world
        .query::<(Entity, &OfProfile)>()
        .iter(world)
        .filter(|(_, of)| of.0 == id)
        .map(|(hire, _)| hire)
        .collect::<Vec<_>>();
    for hire in hires {
        refresh_shown(world, hire);
    }
    entity
}

/// Sets what the agent is shown by from its nickname, its profile's
/// name and its address.
pub(super) fn refresh_shown(world: &mut World, agent: Entity) {
    let Some(address) = world.get::<Name>(agent) else {
        return;
    };
    let shown = world
        .get::<Nickname>(agent)
        .map(|nickname| nickname.0.clone())
        .or_else(|| {
            let of = world.get::<OfProfile>(agent)?;
            let profile =
                world.get::<Profile>(profile(world, of.0)?)?;
            Some(profile.0.name.clone())
        })
        .unwrap_or_else(|| address.as_str().to_owned());
    put(&mut world.entity_mut(agent), Shown(shown));
}

pub(super) fn upsert_agent(
    world: &mut World,
    value: protocol::Agent,
) -> Option<Entity> {
    // A hire whose hamlet has not arrived waits for its next frame.
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
    put(&mut agent, Name::new(value.name));
    flag(&mut agent, value.kind == protocol::AgentKind::Hire, Hire);
    optional(&mut agent, value.parent.map(Parent));
    optional(&mut agent, value.profile.map(OfProfile));
    optional(&mut agent, value.pin.clone().map(Pin));
    optional(&mut agent, value.allowance.map(Allowance));
    optional(&mut agent, value.nickname.clone().map(Nickname));
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
    flag(&mut agent, value.mayor, Mayor);
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

pub(super) fn put<T: Component + PartialEq>(
    entity: &mut EntityWorldMut,
    value: T,
) {
    if entity.get::<T>() != Some(&value) {
        entity.insert(value);
    }
}

/// Puts `value`, or removes the component when there is none.
pub(super) fn optional<T: Component + PartialEq>(
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

pub(super) fn flag<T: Component>(
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

pub(super) fn departures(world: &mut World) {
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
        // agent and look for its roads once more.
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

pub(super) fn society(
    world: &mut World,
    value: protocol::Hamlet,
    news: bool,
) {
    let Some(hamlet) = hamlet(world, &value.id) else {
        return;
    };
    let mut wanted = Vec::new();
    for value in value.roads {
        let (Some(a), Some(b)) =
            (agent(world, value.a), agent(world, value.b))
        else {
            continue;
        };
        // An agent that left has no roads, though its hamlet still
        // lists them.
        if world.get::<Left>(a).is_some()
            || world.get::<Left>(b).is_some()
        {
            continue;
        }
        let ends = if a <= b { (a, b) } else { (b, a) };
        let road = road_between(world, a, b);
        let before = road
            .and_then(|e| world.get::<Traffic>(e))
            .map_or(0, |t| t.messages);
        let traffic = Traffic {
            level: match value.level {
                protocol::RoadLevel::Overgrown => RoadUse::Overgrown,
                protocol::RoadLevel::Dirt => RoadUse::Dirt,
                protocol::RoadLevel::Paved => RoadUse::Paved,
                protocol::RoadLevel::Rail => RoadUse::Rail,
            },
            kind: value.kind,
            messages: value.messages,
            last_from: value
                .last_from
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
    let mut gone = world
        .get::<Agents>(hamlet)
        .into_iter()
        .flat_map(RelationshipTarget::iter)
        .flat_map(|agent| roads_of(world, agent))
        .filter(|road| !wanted.contains(road))
        .collect::<Vec<_>>();
    // A road between two of the hamlet's agents is met at each end.
    gone.sort_unstable();
    gone.dedup();
    for entity in gone {
        world.despawn(entity);
    }
}
