use bevy_app::App;
use bevy_ecs::prelude::*;
use hamlet_client::realm::*;
use hamlet_client::{ClientPlugin, receive};
use hamlet_protocol::client as protocol;
use hamlet_testing::realm::{agent, hamlet, reference, road, state};

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
    let mut removed = agent("home", 1, "scout");
    removed.left = Some(protocol::Left {
        how: protocol::LeftHow::Removed,
        into: None,
        at: 2000,
        by: "chief".into(),
        reason: "done".into(),
        words: "Removed: done".into(),
    });
    let mut merged = agent("home", 2, "smith");
    merged.left = Some(protocol::Left {
        how: protocol::LeftHow::Merged,
        into: Some(reference("home", 3)),
        at: 3000,
        by: "mayor".into(),
        reason: "together".into(),
        words: "Talk to keeper".into(),
    });
    state.agents.extend([
        removed.clone(),
        merged,
        agent("home", 3, "keeper"),
    ]);
    receive(world, protocol::ServerMessage::State(state));
    let gone = world
        .query::<(Entity, &AgentNumber)>()
        .iter(world)
        .map(|(e, c)| (c.0, e))
        .collect::<std::collections::HashMap<_, _>>();
    assert!(world.get::<Left>(gone[&1]).is_some());
    assert_eq!(
        world.get::<Lived>(gone[&1]).unwrap().until,
        Some(2000)
    );
    assert_eq!(
        world.get::<MergedInto>(gone[&2]).unwrap().0,
        gone[&3]
    );
    assert_eq!(named(world, "scout"), None);
    removed.left = None;
    change(
        world,
        1,
        vec![protocol::Update::Agent { agent: removed }],
    );
    assert_eq!(named(world, "scout"), Some(gone[&1]));
    assert!(world.get::<Left>(gone[&1]).is_none());
    assert_eq!(world.get::<Lived>(gone[&1]).unwrap().until, None);
    change(
        world,
        2,
        vec![protocol::Update::AgentGone {
            agent: reference("home", 2),
        }],
    );
    assert!(world.get_entity(gone[&2]).is_err());
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
    state.hamlets.push(hamlet("mill"));
    state.agents.extend([
        agent("home", 1, "scout"),
        agent("mill", 0, "miller"),
    ]);
    let home_road =
        road(reference("home", 0), reference("home", 1), 8);
    let between = road(reference("home", 0), reference("mill", 0), 2);
    let scout_road =
        road(reference("home", 1), reference("mill", 0), 0);
    state.hamlets[0].roads =
        vec![home_road.clone(), between.clone(), scout_road.clone()];
    state.hamlets[1].roads = vec![between.clone(), scout_road];
    receive(world, protocol::ServerMessage::State(state.clone()));
    assert_eq!(world.query::<&Road>().iter(world).count(), 3);
    assert!(world.resource::<Letters>().0.is_empty());
    let founder = named(world, "founder").unwrap();
    let scout = named(world, "scout").unwrap();
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
    roads[0].last_from = Some(reference("home", 1));
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
    state.agents.retain(|a| a.agent.hamlet == "home");
    let miller = named(world, "miller").unwrap();
    let mill = world.get::<LivesIn>(miller).unwrap().0;
    receive(world, protocol::ServerMessage::State(state));
    assert!(world.get_entity(mill).is_err());
    assert!(world.get_entity(miller).is_err());
    assert_eq!(world.query::<&Road>().iter(world).count(), 0);
}

#[derive(Resource, Default)]
struct Ballots(Vec<Entity>);

#[derive(Resource, Default)]
struct Consultations(Vec<Option<Entity>>);

#[test]
fn society_views_reconcile_and_transition_effects_fire_once() {
    let mut app = app();
    app.init_resource::<Ballots>()
        .init_resource::<Consultations>()
        .add_observer(
            |event: On<Voted>, mut ballots: ResMut<Ballots>| {
                ballots.0.push(event.agent);
            },
        )
        .add_observer(
            |event: On<Consulted>,
             mut visits: ResMut<Consultations>| {
                visits.0.push(event.agent);
            },
        );
    let world = app.world_mut();
    let mut state = state();
    state.agents.push(agent("home", 1, "scout"));
    state.hamlets[0].votes = vec![protocol::VoteView {
        id: 7,
        motion: "scout for mayor".into(),
        by: "chief".into(),
        open: true,
        overturnable: false,
        report: "Waiting for scout".into(),
        voters: vec![protocol::Voter {
            agent: reference("home", 1),
            cast: None,
        }],
    }];
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
    let scout = named(world, "scout").unwrap();
    let home = world.get::<LivesIn>(scout).unwrap().0;
    assert_eq!(
        world.get::<Spot>(home).unwrap().0,
        Some((-21.5, 48.0))
    );
    assert_eq!(world.resource::<Skills>().0[0].name, "research");
    assert_eq!(world.resource::<Closed>().0[0].name, "The Mill");
    assert_eq!(world.get::<Votes>(home).unwrap().0[0].id, 7);
    assert!(world.resource::<Ballots>().0.is_empty());
    state.hamlets[0].votes[0].voters[0].cast =
        Some(protocol::VoterCast::Abstain);
    state.agents[1].consulting = true;
    for revision in [1, 2] {
        change(
            world,
            revision,
            vec![
                protocol::Update::Hamlet {
                    hamlet: state.hamlets[0].clone(),
                },
                protocol::Update::Agent {
                    agent: state.agents[1].clone(),
                },
            ],
        );
    }
    assert_eq!(world.resource::<Ballots>().0, [scout]);
    assert_eq!(world.resource::<Consultations>().0, [Some(scout)]);
    state.revision = 3;
    state.library = Vec::new();
    state.closed = Vec::new();
    state.hamlets[0].votes = Vec::new();
    state.hamlets[0].at = None;
    state.agents[1].consulting = false;
    receive(world, protocol::ServerMessage::State(state));
    assert!(world.resource::<Closed>().0.is_empty());
    assert!(world.resource::<Skills>().0.is_empty());
    assert!(world.get::<Votes>(home).unwrap().0.is_empty());
    assert_eq!(world.get::<Spot>(home).unwrap().0, None);
}
