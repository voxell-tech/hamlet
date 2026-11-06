use bevy_app::App;
use bevy_ecs::prelude::*;
use hamlet_client::realm::*;
use hamlet_client::{ClientPlugin, receive};
use hamlet_protocol::client as protocol;
use hamlet_testing::realm::{agent, hamlet, road, state};

fn app() -> App {
    let mut app = App::new();
    app.add_plugins(ClientPlugin { server: None });
    app
}

fn change(
    world: &mut World,
    revision: i64,
    changes: Vec<protocol::Update>,
) {
    receive(
        world,
        protocol::ServerMessage::Change(protocol::Changes {
            epoch: 1,
            revision,
            changes,
        }),
    );
}

#[test]
fn departures_resolve_later_merge_hosts_and_restores_keep_identity() {
    let mut app = app();
    let world = app.world_mut();
    let mut state = state();
    let mut removed = agent(3, "home", 1);
    removed.left = Some(protocol::Left {
        how: protocol::LeftHow::Ended,
        into: None,
        at: 2000,
        by: "chief".into(),
        reason: "done".into(),
        words: "Removed: done".into(),
    });
    let mut merged = agent(4, "home", 2);
    merged.left = Some(protocol::Left {
        how: protocol::LeftHow::Merged,
        into: Some(protocol::AgentId(5)),
        at: 3000,
        by: "mayor".into(),
        reason: "together".into(),
        words: "Talk to keeper".into(),
    });
    state.agents.extend([
        removed.clone(),
        merged,
        agent(5, "home", 3),
    ]);
    receive(world, protocol::ServerMessage::State(state));
    let gone = world
        .query::<(Entity, &Identity)>()
        .iter(world)
        .map(|(e, c)| (c.0.0, e))
        .collect::<std::collections::HashMap<_, _>>();
    assert!(world.get::<Left>(gone[&3]).is_some());
    assert_eq!(
        world.get::<Lived>(gone[&3]).unwrap().until,
        Some(2000)
    );
    assert_eq!(
        world.get::<MergedInto>(gone[&4]).unwrap().0,
        gone[&5]
    );
    assert_eq!(named(world, "home#1"), None);
    removed.left = None;
    change(
        world,
        1,
        vec![protocol::Update::Agent { agent: removed }],
    );
    assert_eq!(named(world, "home#1"), Some(gone[&3]));
    assert!(world.get::<Left>(gone[&3]).is_none());
    assert_eq!(world.get::<Lived>(gone[&3]).unwrap().until, None);
    change(
        world,
        2,
        vec![protocol::Update::AgentGone {
            id: protocol::AgentId(4),
        }],
    );
    assert!(world.get_entity(gone[&4]).is_err());
}

#[derive(Resource, Default)]
struct Letters(Vec<(Entity, Entity)>);

#[test]
fn roads_are_unordered_shared_and_only_growth_carries_letters() {
    let mut app = app();
    app.init_resource::<Letters>().add_observer(
        |event: On<Carried>, mut letters: ResMut<Letters>| {
            letters.0.push((event.road, event.from));
        },
    );
    let world = app.world_mut();
    let mut state = state();
    state.hamlets.push(hamlet("mill", 4));
    state
        .agents
        .extend([agent(3, "home", 1), agent(4, "mill", 0)]);
    let [ada, scout_id, miller] = [2, 3, 4].map(protocol::AgentId);
    let home_road = road(ada, scout_id, 8);
    let between = road(ada, miller, 2);
    let scout_road = road(scout_id, miller, 0);
    state.hamlets[0].roads =
        vec![home_road.clone(), between.clone(), scout_road.clone()];
    state.hamlets[1].roads = vec![between.clone(), scout_road];
    receive(world, protocol::ServerMessage::State(state.clone()));
    assert_eq!(world.query::<&Road>().iter(world).count(), 3);
    assert!(world.resource::<Letters>().0.is_empty());
    let founder = named(world, "home#0").unwrap();
    let scout = named(world, "home#1").unwrap();
    let entity = world
        .query::<(Entity, &Road)>()
        .iter(world)
        .find(|(_, r)| r.has_end(founder) && r.has_end(scout))
        .unwrap()
        .0;
    let mut changed = state.hamlets[0].clone();
    let roads = &mut changed.roads;
    let first = &mut roads[0];
    std::mem::swap(&mut first.a, &mut first.b);
    roads[0].messages += 1;
    roads[0].last_from = Some(scout_id);
    change(
        world,
        1,
        vec![protocol::Update::Hamlet {
            hamlet: changed.clone(),
        }],
    );
    assert_eq!(world.resource::<Letters>().0, [(entity, scout)]);
    assert_eq!(
        world.get::<Traffic>(entity).unwrap().level,
        RoadUse::Paved
    );
    change(
        world,
        2,
        vec![protocol::Update::Hamlet {
            hamlet: changed.clone(),
        }],
    );
    assert_eq!(world.resource::<Letters>().0.len(), 1);
    changed.roads = vec![between];
    change(
        world,
        3,
        vec![protocol::Update::Hamlet { hamlet: changed }],
    );
    assert_eq!(world.query::<&Road>().iter(world).count(), 1);
    assert!(world.get_entity(entity).is_err());
    state.revision = 4;
    state.hamlets.truncate(1);
    state.hamlets[0].roads = Vec::new();
    state.agents.retain(|a| a.hamlet.as_deref() != Some("mill"));
    let miller = named(world, "mill#0").unwrap();
    let mill = world.get::<LivesIn>(miller).unwrap().0;
    receive(world, protocol::ServerMessage::State(state));
    assert!(world.get_entity(mill).is_err());
    assert!(world.get_entity(miller).is_err());
    assert_eq!(world.query::<&Road>().iter(world).count(), 0);
}

#[derive(Resource, Default)]
struct Consultations(Vec<Option<Entity>>);

#[test]
fn society_views_reconcile_and_transition_effects_fire_once() {
    let mut app = app();
    app.init_resource::<Consultations>().add_observer(
        |event: On<Consulted>, mut visits: ResMut<Consultations>| {
            visits.0.push(event.agent);
        },
    );
    let world = app.world_mut();
    let mut state = state();
    state.agents.push(agent(3, "home", 1));
    state.hamlets[0].at =
        Some(protocol::TownAt { x: -21.5, z: 48.0 });
    state.library = vec![protocol::LibrarySkill {
        name: "research".into(),
        description: "Read sources".into(),
        by: "scout".into(),
    }];
    state.closed = vec![protocol::ClosedHamlet {
        id: "mill".into(),
        name: "The Mill".into(),
        at: 99,
        by: "chief".into(),
        reason: "quiet".into(),
    }];
    receive(world, protocol::ServerMessage::State(state.clone()));
    let scout = named(world, "home#1").unwrap();
    let home = world.get::<LivesIn>(scout).unwrap().0;
    assert_eq!(
        world.get::<Spot>(home).unwrap().0,
        Some((-21.5, 48.0))
    );
    assert_eq!(world.resource::<Skills>().0[0].name, "research");
    assert_eq!(world.resource::<Closed>().0[0].name, "The Mill");
    state.agents[2].consulting = true;
    for revision in [1, 2] {
        change(
            world,
            revision,
            vec![
                protocol::Update::Hamlet {
                    hamlet: state.hamlets[0].clone(),
                },
                protocol::Update::Agent {
                    agent: state.agents[2].clone(),
                },
            ],
        );
    }
    assert_eq!(world.resource::<Consultations>().0, [Some(scout)]);
    state.revision = 3;
    state.library = Vec::new();
    state.closed = Vec::new();
    state.hamlets[0].at = None;
    state.agents[2].consulting = false;
    receive(world, protocol::ServerMessage::State(state));
    assert!(world.resource::<Closed>().0.is_empty());
    assert!(world.resource::<Skills>().0.is_empty());
    assert_eq!(world.get::<Spot>(home).unwrap().0, None);
}

#[test]
fn the_hand_scholars_and_workers_are_agents_and_scholars_of_their_own()
 {
    let mut app = app();
    let world = app.world_mut();
    let mut state = state();
    let mut worker = agent(3, "home", 1);
    worker.kind = protocol::AgentKind::Worker;
    worker.name = "worker#3".into();
    worker.hamlet = None;
    worker.number = None;
    worker.scholar = None;
    worker.parent = None;
    state.agents.push(worker);
    receive(world, protocol::ServerMessage::State(state.clone()));
    let hand = hand(world).unwrap();
    assert_eq!(
        world.get::<Kind>(hand).unwrap().0,
        protocol::AgentKind::Hand
    );
    assert!(world.get::<LivesIn>(hand).is_none());
    assert!(world.get::<AgentNumber>(hand).is_none());
    let worker = named(world, "worker#3").unwrap();
    assert_eq!(
        world.get::<Kind>(worker).unwrap().0,
        protocol::AgentKind::Worker
    );
    assert!(world.get::<LivesIn>(worker).is_none());
    let ada = scholar(world, protocol::ScholarId(2)).unwrap();
    assert_eq!(world.get::<Name>(ada).unwrap().as_str(), "ada");
    assert_eq!(
        world.get::<Scholar>(ada).unwrap().0.hires,
        [protocol::AgentId(2)]
    );
    let mayor = mayor_of(world, "home").unwrap();
    assert_eq!(world.get::<AgentNumber>(mayor).unwrap().0, 0);
    state.revision = 1;
    state.scholars.clear();
    receive(world, protocol::ServerMessage::State(state));
    assert!(world.get_entity(ada).is_err());
}
