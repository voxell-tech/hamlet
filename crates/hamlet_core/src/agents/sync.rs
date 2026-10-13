//! The records made into entities: `sync` for hamlets, agents and
//! roads, `account` for spend, and `watch` to run them when a file
//! changes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use bevy_ecs::name::Name;
use bevy_ecs::prelude::*;
use hamlet_home::limits;
use hamlet_rules::budget::{self, Ledger};
use hamlet_rules::hamlet::{self, HOME, MAYOR};
use hamlet_rules::realm::{self, Realm};

use super::roster::settlements;
use super::{
    Account, Activity, Busy, Carried, Charter, Citizen, First, Left,
    Lived, LivesIn, Mayor, MergedInto, Purpose, Road, Settlement,
    Spot, Synced, Traffic, Treasury, Works,
};
use crate::core::Settings;

/// The sizes and times of a hamlet's files when its entities were
/// last brought in step with them.
#[derive(Component, Default, Debug, PartialEq)]
pub struct Seen {
    /// The length of the record, which only grows.
    pub(super) record: u64,
    /// The length of the realm's record, which only grows.
    realm: u64,
    /// The length of today's ledger.
    spend: u64,
    /// The time the limits were last written, and their length.
    limits: Option<(SystemTime, u64)>,
}

impl Seen {
    fn read(settlement: &Settlement) -> Self {
        let length = |path: PathBuf| {
            std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
        };
        let hamlet = settlement.record();
        let root = hamlet.root();
        Seen {
            record: length(hamlet.record()),
            realm: length(Realm::path(&settlement.home)),
            spend: length(Ledger::new(root).file(budget::today())),
            limits: Seen::limits(root),
        }
    }

    /// The time the limits of the hamlet at `root` were last written,
    /// and their length.
    fn limits(root: &Path) -> Option<(SystemTime, u64)> {
        let file = std::fs::metadata(limits::path(root)).ok()?;
        Some((file.modified().ok()?, file.len()))
    }
}

pub(super) fn found(world: &mut World, home: &Path, name: &str) {
    let settlement = Settlement {
        home: home.to_path_buf(),
        name: name.to_string(),
    };
    // A change to the hamlet that a crash cut short is finished
    // before anyone reads it.
    settlement.record().repair();
    let hamlet =
        world.spawn((Name::new(name.to_string()), settlement)).id();
    if name == HOME {
        world.insert_resource(First(hamlet));
    }
}

/// The time between two looks at the hamlets' files for a change.
/// Zero looks on every update.
#[derive(Resource, Clone, Copy, Default, Debug, PartialEq)]
pub struct WatchEvery(pub Duration);

/// Queues [`sync`] or [`account`] when a hamlet's files are not as
/// they were last read.
pub fn watch(
    every: Option<Res<WatchEvery>>,
    settlements: Query<(&Settlement, &Seen)>,
    mut looked: Local<Option<Instant>>,
    mut commands: Commands,
) {
    let every = every.map_or(Duration::ZERO, |every| every.0);
    if looked.is_some_and(|at| at.elapsed() < every) {
        return;
    }
    *looked = Some(Instant::now());
    let mut spent = false;
    for (settlement, seen) in &settlements {
        let now = Seen::read(settlement);
        if (now.record, now.realm) != (seen.record, seen.realm) {
            commands.queue(sync);
            return;
        }
        spent |= now != *seen;
    }
    if spent {
        commands.queue(account);
    }
}

/// An agent as the record tells it.
struct Told {
    number: u64,
    name: String,
    purpose: String,
    mayor: bool,
    /// `Some` when it left; inside, the number of the agent it was
    /// merged into.
    left: Option<Option<u64>>,
    lived: Lived,
}

/// Brings the hamlet entities in step with the realm's record.
fn settle(world: &mut World) {
    let known = settlements(world);
    let Some(home) = known.first().map(|(_, s)| s.home.clone())
    else {
        return;
    };
    let places = Realm::new(home.clone()).hamlets();
    for (entity, settlement) in &known {
        if !places.iter().any(|p| p.name == settlement.name) {
            world.entity_mut(*entity).despawn();
        }
    }
    for place in &places {
        if !known.iter().any(|(_, s)| s.name == place.name) {
            found(world, &home, &place.name);
        }
    }
    // Each is shown by what its chief calls it.
    for (entity, settlement) in settlements(world) {
        let Some(place) =
            places.iter().find(|p| p.name == settlement.name)
        else {
            continue;
        };
        let mut hamlet = world.entity_mut(entity);
        if hamlet
            .get::<Name>()
            .is_none_or(|name| name.as_str() != place.shown())
        {
            hamlet.insert(Name::new(place.shown().to_string()));
        }
        // And stands where its chief set it down.
        let spot = Spot(place.at);
        if hamlet.get::<Spot>() != Some(&spot) {
            hamlet.insert(spot);
        }
    }
}

/// Brings every hamlet, its agents and the roads in step with the
/// records, then the accounts.
pub fn sync(world: &mut World) {
    settle(world);
    charter(world);
    let grows_over = grows_over(world);
    let mut wanted = Wanted {
        was: world.remove_resource::<Borne>(),
        ..Wanted::default()
    };
    // Every living agent by its address from outside its hamlet.
    let mut abroad = HashMap::<String, Entity>::new();
    let mut home = None;
    for (hamlet, settlement) in settlements(world) {
        // Read before the records, so a line written meanwhile is
        // seen as growth next time.
        let seen = Seen::read(&settlement);
        let census = settlement.record().census();
        let works = Works(settlement.record().work());
        if world.get::<Works>(hamlet) != Some(&works) {
            world.entity_mut(hamlet).insert(works);
        }
        let told = std::iter::once(Told {
            number: census.mayor.id,
            name: census.mayor.name.clone(),
            purpose: census.mayor.purpose.clone(),
            mayor: true,
            left: None,
            lived: Lived::default(),
        })
        .chain(census.residents.iter().map(|r| Told {
            number: r.id,
            name: r.name.clone(),
            purpose: r.purpose.clone(),
            mayor: false,
            left: None,
            lived: Lived {
                since: r.since,
                until: None,
            },
        }))
        .chain(
            census.archive.iter().filter(|a| a.back.is_none()).map(
                |a| Told {
                    number: a.resident.id,
                    name: a.resident.name.clone(),
                    purpose: a.resident.purpose.clone(),
                    mayor: false,
                    left: Some(a.merge.as_ref().map(|m| m.kept.id)),
                    lived: Lived {
                        since: a.resident.since,
                        until: Some(a.left),
                    },
                },
            ),
        )
        .collect::<Vec<_>>();

        let mut agents = world
            .query::<(Entity, &Citizen, &LivesIn)>()
            .iter(world)
            .filter(|(_, _, lives)| lives.0 == hamlet)
            .map(|(entity, citizen, _)| (citizen.0, entity))
            .collect::<HashMap<_, _>>();
        let stale = agents
            .iter()
            .filter(|(number, _)| {
                !told.iter().any(|t| t.number == **number)
            })
            .map(|(number, entity)| (*number, *entity))
            .collect::<Vec<_>>();
        for (number, entity) in stale {
            world.entity_mut(entity).despawn();
            agents.remove(&number);
        }
        for t in &told {
            let entity =
                *agents.entry(t.number).or_insert_with(|| {
                    world
                        .spawn((Citizen(t.number), LivesIn(hamlet)))
                        .id()
                });
            let mut agent = world.entity_mut(entity);
            match (t.mayor, agent.contains::<Mayor>()) {
                (true, false) => {
                    agent.insert(Mayor);
                }
                (false, true) => {
                    agent.remove::<Mayor>();
                }
                _ => {}
            }
            if agent
                .get::<Name>()
                .is_none_or(|n| n.as_str() != t.name)
            {
                agent.insert(Name::new(t.name.clone()));
            }
            if agent.get::<Purpose>().is_none_or(|p| p.0 != t.purpose)
            {
                agent.insert(Purpose(t.purpose.clone()));
            }
        }
        for t in &told {
            let mut agent = world.entity_mut(agents[&t.number]);
            if agent.get::<Lived>() != Some(&t.lived) {
                agent.insert(t.lived);
            }
            let gone = agent.contains::<Left>();
            let into = agent.get::<MergedInto>().map(|m| m.0);
            match t.left {
                None if gone => {
                    agent.remove::<(Left, MergedInto)>();
                }
                None => {}
                Some(host) => {
                    let host =
                        host.and_then(|h| agents.get(&h).copied());
                    if !gone {
                        agent
                            .remove::<Busy>()
                            .insert(Activity::default());
                    }
                    match host {
                        Some(host) if !gone || into != Some(host) => {
                            agent.insert((Left, MergedInto(host)));
                        }
                        Some(_) => {}
                        None => {
                            agent.remove::<MergedInto>();
                            if !gone {
                                agent.insert(Left);
                            }
                        }
                    }
                }
            }
        }

        // The mayor is reached at the office, the others by name.
        let living = told
            .iter()
            .filter(|t| t.left.is_none())
            .map(|t| {
                let address =
                    if t.mayor { MAYOR } else { t.name.as_str() };
                (address, agents[&t.number])
            })
            .collect::<HashMap<_, _>>();
        wanted.add(
            &settlement.name,
            &census.roads,
            grows_over,
            |address| living.get(address).copied(),
        );
        for t in told.iter().filter(|t| t.left.is_none()) {
            let agent = agents[&t.number];
            abroad.insert(t.name.clone(), agent);
            if t.mayor {
                abroad.insert(realm::office(&settlement.name), agent);
            }
        }
        if let Some(mut was) = world.get_mut::<Seen>(hamlet) {
            was.record = seen.record;
            was.realm = seen.realm;
        }
        home = Some(settlement.home);
    }
    if let Some(home) = home {
        let between = Realm::new(home).roads();
        wanted.add("", &between, grows_over, |address| {
            abroad.get(address).copied()
        });
    }

    let mut carried = Vec::new();
    let mut roads = world
        .query::<(Entity, &Road, &Traffic)>()
        .iter(world)
        .map(|(entity, road, traffic)| {
            (road.ends, (entity, *traffic))
        })
        .collect::<HashMap<_, _>>();
    for (ends, traffic) in wanted.roads {
        let road = match roads.remove(&ends) {
            None => world.spawn((Road { ends }, traffic)).id(),
            Some((entity, was)) => {
                if was != traffic {
                    world.entity_mut(entity).insert(traffic);
                }
                entity
            }
        };
        if let (true, Some(from)) =
            (wanted.used.contains(&ends), traffic.last_from)
        {
            carried.push(Carried { road, from });
        }
    }
    for (entity, _) in roads.into_values() {
        world.entity_mut(entity).despawn();
    }
    world.insert_resource(wanted.borne);
    account(world);
    for event in carried {
        world.trigger(event);
    }
    world.trigger(Synced);
    world.flush();
}

/// Days without use after which a road is grown over, as the
/// limits of the first hamlet have it.
fn grows_over(world: &World) -> u64 {
    super::limits(world)
        .unwrap_or_default()
        .road
        .grows_over_days
}

/// A road as the records name it: its hamlet, or nothing for a road
/// between hamlets, and its two ends.
type Named = (String, (String, String));

/// The messages each road of the records had carried when the
/// entities were last brought in step with them.
#[derive(Resource, Default)]
struct Borne(HashMap<Named, u32>);

/// The roads the records have, as one sync gathers them.
#[derive(Default)]
struct Wanted {
    /// Each road by the agents at its ends.
    roads: HashMap<(Entity, Entity), Traffic>,
    /// The roads that carried a message since the sync before.
    used: Vec<(Entity, Entity)>,
    borne: Borne,
    /// Nothing at the first sync, when no message is news.
    was: Option<Borne>,
}

impl Wanted {
    /// Adds the `roads` of `place`, each end found through `agent`.
    /// A road with an end nobody answers to is left out, and one
    /// unused for `grows_over` days is grown over.
    fn add(
        &mut self,
        place: &str,
        roads: &[hamlet::Road],
        grows_over: u64,
        agent: impl Fn(&str) -> Option<Entity>,
    ) {
        for road in roads {
            let named = (place.to_string(), road.ends.clone());
            let rose = self.was.as_ref().is_some_and(|was| {
                let before = was.0.get(&named).copied().unwrap_or(0);
                road.messages > before
            });
            self.borne.0.insert(named, road.messages);
            let (Some(a), Some(b)) =
                (agent(&road.ends.0), agent(&road.ends.1))
            else {
                continue;
            };
            let ends = if a <= b { (a, b) } else { (b, a) };
            if rose {
                self.used.push(ends);
            }
            self.roads.insert(
                ends,
                Traffic {
                    level: road.level(grows_over),
                    messages: road.messages,
                    last_from: road
                        .last_from
                        .as_deref()
                        .and_then(&agent),
                },
            );
        }
    }
}

/// Reads again the limits of each hamlet whose file is not as it
/// was last read.
fn charter(world: &mut World) {
    for (hamlet, settlement) in settlements(world) {
        // Read before the file, so a change made meanwhile is seen
        // next time.
        let now = Seen::limits(settlement.record().root());
        let mut hamlet = world.entity_mut(hamlet);
        let read = hamlet.contains::<Charter>()
            && hamlet.get::<Seen>().is_some_and(|s| s.limits == now);
        if read {
            continue;
        }
        hamlet.insert(Charter(settlement.record().limits().ok()));
        if let Some(mut seen) = hamlet.get_mut::<Seen>() {
            seen.limits = now;
        }
    }
}

/// Brings every hamlet's spend and models in step with the ledger,
/// the record and the limits.
pub fn account(world: &mut World) {
    charter(world);
    let own_model = world
        .get_resource::<Settings>()
        .map(|s| s.config.model.clone())
        .unwrap_or_default();
    for (hamlet, settlement) in settlements(world) {
        let seen = Seen::read(&settlement);
        let mut report = settlement.record().report(&own_model);
        let agents = world
            .query_filtered::<(Entity, &Name, &LivesIn, Has<Left>, Has<Mayor>), With<Citizen>>()
            .iter(world)
            .filter(|(_, _, lives, ..)| lives.0 == hamlet)
            .map(|(entity, name, _, left, mayor)| {
                // The mayor's spend is the office's.
                let address =
                    if mayor { MAYOR } else { name.as_str() };
                let line =
                    report.agent(address).filter(|_| !left).cloned();
                (entity, line)
            })
            .collect::<Vec<_>>();
        for (entity, line) in agents {
            let mut agent = world.entity_mut(entity);
            let was = agent.get::<Account>().map(|a| &a.0);
            match line {
                Some(line) if was != Some(&line) => {
                    agent.insert(Account(line));
                }
                None if was.is_some() => {
                    agent.remove::<Account>();
                }
                _ => {}
            }
        }
        report.agents.clear();
        let mut hamlet = world.entity_mut(hamlet);
        if hamlet.get::<Treasury>().is_none_or(|t| t.0 != report) {
            hamlet.insert(Treasury(report));
        }
        if let Some(mut was) = hamlet.get_mut::<Seen>() {
            was.spend = seen.spend;
        }
    }
}
