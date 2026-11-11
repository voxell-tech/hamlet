use bevy::prelude::*;
use hamlet_client::realm::{
    Activity, Busy, Carried, Doing, HamletId, Hire, Identity, Kind,
    Left, Lived, LivesIn, Mayor, MergedInto, Road, RoadUse, Spot,
    Traffic,
};
use hamlet_client::{ClientPlugin, receive};
use hamlet_protocol::client as protocol;
use hamlet_testing::realm;

use super::{Housed, Laid, Returning, Sited, Unhoused, town_of};
use crate::generator::land::Land;
use crate::generator::society::{
    Agent, Ground, Pair, RoadLevel, Society,
};
use crate::generator::{
    Ahead, GeneratorView, Objecting, Trips, testing,
};
use crate::keeper::KeeperState;
use crate::keeper::work::Task;
use crate::pick::{Centre, Glide, Leash, Orbit, Stance};
use crate::sky::CameraSettings;
use crate::testing::{receive_change, receive_state};
use crate::{Clock, Stage};

/// The first hamlet's id in the client's fixture.
const HOME: &str = "home";

/// The id of a test hire, by the order the tests count it in: 0 is
/// the fixture's mayor, the rest are made by [`hire`].
fn id_of(number: u64) -> protocol::AgentId {
    protocol::AgentId(if number == 0 { 2 } else { 1000 + number })
}

/// A hire of a test hamlet, counted `number`.
fn hire(number: u64, hamlet: Entity) -> impl Bundle {
    (
        Identity(id_of(number)),
        Kind(protocol::AgentKind::Hire),
        Hire,
        LivesIn(hamlet),
    )
}

/// The world's side of the hamlets, with no hamlet in it: the real
/// plugin on the client's entities. The resources it reads are
/// inserted by hand: the plugins that own them need Bevy's default
/// plugins (generator, pick and sky) or move the clock each frame
/// (stage).
fn bare() -> App {
    let mut app = App::new();
    app.add_plugins(ClientPlugin { server: None })
        .init_resource::<Stage>()
        .init_resource::<Society>()
        .init_resource::<Trips>()
        .init_resource::<Objecting>()
        .init_resource::<Clock>()
        .insert_resource(testing::land(1))
        .init_resource::<CameraSettings>()
        .init_resource::<Glide>()
        .init_resource::<Orbit>()
        .init_resource::<Leash>()
        .init_resource::<Centre>()
        .init_resource::<Stance>()
        .init_resource::<GeneratorView>()
        .init_resource::<Ahead>()
        .add_plugins(super::plugin);
    app
}

fn app() -> (App, Entity) {
    let mut app = bare();
    let hamlet = settle(&mut app, HOME);
    (app, hamlet)
}

#[test]
fn a_hamlet_that_is_removed_takes_its_town_with_it() {
    let (mut app, _) = app();
    let mill = settle(&mut app, "mill");
    let miller =
        app.world_mut().spawn((hire(1, mill), Lived::default()));
    let miller = miller.id();
    app.update();
    assert_eq!(app.world().resource::<Society>().towns.len(), 2);
    let world = app.world_mut();
    let of_mill = world
        .query::<(Entity, &LivesIn)>()
        .iter(world)
        .filter(|(_, lives)| lives.0 == mill)
        .map(|(entity, _)| entity)
        .collect::<Vec<_>>();
    assert!(of_mill.contains(&miller));
    for agent in of_mill {
        world.entity_mut(agent).despawn();
    }
    world.entity_mut(mill).despawn();
    app.update();
    let society = app.world().resource::<Society>();
    assert_eq!(society.towns.len(), 1);
    let town = society.towns[0].id;
    assert!(society.agents.iter().all(|a| a.town == town));
    assert!(society.roads.is_empty());
}

/// A hamlet with its first mayor in office.
fn settle(app: &mut App, name: &str) -> Entity {
    if app
        .world()
        .contains_resource::<hamlet_client::realm::Seen>()
    {
        // The mayors take the ids after the Hand's and the first
        // mayor's.
        let world = app.world_mut();
        let mayor =
            world.query::<&HamletId>().iter(world).count() as u64 + 2;
        receive_change(
            app,
            protocol::Update::Hamlet {
                hamlet: realm::hamlet(name, mayor),
            },
        );
        receive_change(
            app,
            protocol::Update::Agent {
                agent: realm::agent(mayor, name, true),
            },
        );
    } else {
        receive_state(app, realm::state());
    }
    let world = app.world_mut();
    world
        .query::<(Entity, &HamletId)>()
        .iter(world)
        .find(|(_, hamlet)| hamlet.0 == name)
        .unwrap()
        .0
}

fn entity(app: &mut App, number: u64) -> Option<Entity> {
    let world = app.world_mut();
    let first = world
        .query::<(Entity, &HamletId)>()
        .iter(world)
        .find(|(_, s)| s.0 == HOME)
        .map(|(entity, _)| entity)?;
    world
        .query::<(Entity, &Identity, &LivesIn)>()
        .iter(world)
        .find(|(_, id, lives)| {
            id.0 == id_of(number) && lives.0 == first
        })
        .map(|(entity, ..)| entity)
}

/// Sets who lives in the hamlet and who left, each by its
/// number, and runs a frame.
fn tell(
    app: &mut App,
    hamlet: Entity,
    living: &[u64],
    gone: &[(u64, Option<u64>)],
) {
    for number in living.iter().chain(gone.iter().map(|(n, _)| n)) {
        if entity(app, *number).is_none() {
            app.world_mut().spawn(hire(*number, hamlet));
        }
    }
    for number in living {
        let agent = entity(app, *number).unwrap();
        app.world_mut()
            .entity_mut(agent)
            .remove::<(Left, MergedInto)>();
    }
    for (number, into) in gone {
        let agent = entity(app, *number).unwrap();
        if app.world().get::<Left>(agent).is_some() {
            continue;
        }
        let host = into.map(|into| entity(app, into).unwrap());
        let mut agent = app.world_mut().entity_mut(agent);
        match host {
            Some(host) => {
                agent.insert((Left, MergedInto(host)));
            }
            None => {
                agent.insert(Left);
            }
        }
    }
    app.update();
}

fn agent(app: &mut App, number: u64) -> Agent {
    let agent = entity(app, number).unwrap();
    let housed = app.world().get::<Housed>(agent).unwrap().0;
    let society = app.world().resource::<Society>();
    society.agent(housed).unwrap().clone()
}

#[test]
fn the_world_shows_who_left_and_who_came_back() {
    let (mut app, hamlet) = app();
    tell(&mut app, hamlet, &[1, 2, 3], &[]);
    let plots = [1, 2, 3].map(|id| agent(&mut app, id).plot);

    // Removed: a ruin.
    tell(&mut app, hamlet, &[1, 3], &[(2, None)]);
    assert!(agent(&mut app, 2).archived);

    // Merged: a ruin, and a wing on the house that took it in.
    tell(&mut app, hamlet, &[1], &[(2, None), (3, Some(1))]);
    assert!(agent(&mut app, 3).archived);
    assert_eq!(agent(&mut app, 1).wings, 1);

    // Split and restored: both stand again where they stood.
    tell(&mut app, hamlet, &[1, 3, 2], &[]);
    assert_eq!(agent(&mut app, 1).wings, 0);
    for (id, plot) in [1, 2, 3].into_iter().zip(plots) {
        let now = agent(&mut app, id);
        assert!(!now.archived);
        assert_eq!(now.plot, plot);
    }
    let world = app.world_mut();
    let returning = world
        .query_filtered::<(), With<Returning>>()
        .iter(world)
        .count();
    assert_eq!(returning, 2);
}

#[test]
fn a_new_mayor_trades_houses_with_the_one_before() {
    let (mut app, hamlet) = app();
    tell(&mut app, hamlet, &[1, 2], &[]);
    let (hall, home) =
        (agent(&mut app, 0).plot, agent(&mut app, 2).plot);
    let (founder, two) =
        (entity(&mut app, 0).unwrap(), entity(&mut app, 2).unwrap());
    app.world_mut().entity_mut(founder).remove::<Mayor>();
    app.world_mut().entity_mut(two).insert(Mayor);
    app.update();
    assert_eq!(agent(&mut app, 2).plot, hall);
    assert_eq!(agent(&mut app, 0).plot, home);
    let town = app.world().get::<Sited>(hamlet).unwrap().0;
    let mayor =
        app.world().resource::<Society>().town(town).unwrap().mayor;
    assert_eq!(mayor, agent(&mut app, 2).id);
}

#[test]
fn a_world_opened_later_puts_everyone_on_the_same_plot() {
    let (mut first, hamlet) = app();
    tell(&mut first, hamlet, &[1, 2, 3], &[]);
    let (mut later, hamlet) = app();
    tell(&mut later, hamlet, &[1], &[(2, None), (3, Some(1))]);
    for id in [1, 2, 3] {
        assert_eq!(
            agent(&mut later, id).plot,
            agent(&mut first, id).plot
        );
    }
    assert!(
        agent(&mut later, 2).archived
            && agent(&mut later, 3).archived
    );
    assert_eq!(agent(&mut later, 1).wings, 1);
}

#[test]
fn a_newcomer_takes_the_plot_of_one_who_left_and_keeps_it() {
    // Two is removed before three comes, so three builds where
    // two stood, in a world that sees it happen and in one that
    // is opened afterward.
    let lived =
        |since: u64, until: Option<u64>| Lived { since, until };
    let (mut seen, hamlet) = app();
    tell(&mut seen, hamlet, &[1, 2], &[]);
    let stood = agent(&mut seen, 2).plot;
    tell(&mut seen, hamlet, &[1], &[(2, None)]);
    tell(&mut seen, hamlet, &[1, 3], &[(2, None)]);
    assert_eq!(agent(&mut seen, 3).plot, stood);
    assert!(agent(&mut seen, 2).cleared);

    let (mut later, hamlet) = app();
    for (number, times) in [
        (1, lived(10, None)),
        (2, lived(20, Some(30))),
        (3, lived(40, None)),
    ] {
        later.world_mut().spawn((hire(number, hamlet), times));
    }
    tell(&mut later, hamlet, &[1, 3], &[(2, None)]);
    assert_eq!(agent(&mut later, 3).plot, stood);
    assert!(agent(&mut later, 2).cleared);
    assert_eq!(agent(&mut later, 1).plot, agent(&mut seen, 1).plot);
}

#[test]
fn an_agent_with_no_plot_waits_for_one_and_the_society_rests() {
    let (mut app, hamlet) = app();
    let plots = app.world().resource::<Land>().set.town.plots;
    let all = (1..=plots as u64 + 1).collect::<Vec<_>>();
    tell(&mut app, hamlet, &all, &[]);
    let waiting = |app: &mut App| {
        let world = app.world_mut();
        world
            .query_filtered::<(), With<Unhoused>>()
            .iter(world)
            .count()
    };
    let left_out = waiting(&mut app);
    assert!(left_out > 0);
    app.update();
    assert!(!app.world().is_resource_changed::<Society>());

    // One leaves, and one of those waiting takes its plot.
    tell(&mut app, hamlet, &all[1..], &[(1, None)]);
    assert_eq!(waiting(&mut app), left_out - 1);
}

#[test]
fn roads_and_work_follow_the_entities() {
    let (mut app, hamlet) = app();
    tell(&mut app, hamlet, &[1, 2], &[]);
    let (a, b) =
        (entity(&mut app, 1).unwrap(), entity(&mut app, 2).unwrap());
    let traffic = |messages, level| Traffic {
        kind: protocol::RoadKind::Laid,
        level,
        messages,
        last_from: Some(a),
    };
    let road = app
        .world_mut()
        .spawn((Road { ends: (a, b) }, traffic(0, RoadUse::Dirt)))
        .id();
    app.world_mut().entity_mut(a).insert((
        Busy,
        Activity {
            doing: Doing::Fetch,
            step: 2,
            failures: 0,
            ..default()
        },
    ));
    app.update();
    let pair = app.world().get::<Laid>(road).unwrap().0;
    let level = |app: &App| {
        app.world()
            .resource::<Society>()
            .road(pair)
            .map(|r| r.level)
    };
    assert_eq!(level(&app), Some(RoadLevel::Dirt));
    let one = agent(&mut app, 1);
    assert_eq!(one.state, KeeperState::Working);
    assert_eq!(one.work.task, Task::Fetch);

    // A message: the road is used more and its sender walks.
    app.world_mut()
        .entity_mut(road)
        .insert(traffic(3, RoadUse::Paved));
    app.world_mut().trigger(Carried { road, from: a });
    app.update();
    assert_eq!(level(&app), Some(RoadLevel::Paved));
    let trips = &app.world().resource::<Trips>().trips;
    assert_eq!(trips.len(), 1);
    assert_eq!(trips[0].walker, one.id);

    app.world_mut().entity_mut(road).despawn();
    assert_eq!(level(&app), None);
}

#[test]
fn several_hamlets_are_towns_with_roads_between() {
    let (mut app, hamlet) = app();
    let mill = settle(&mut app, "mill");
    tell(&mut app, hamlet, &[1, 2], &[]);
    let miller = {
        let world = app.world_mut();
        world
            .query::<(Entity, &LivesIn)>()
            .iter(world)
            .find(|(_, lives)| lives.0 == mill)
            .map(|(entity, _)| entity)
            .unwrap()
    };
    let housed = |app: &App, agent: Entity| {
        let id = app.world().get::<Housed>(agent).unwrap().0;
        app.world().resource::<Society>().agent(id).unwrap().clone()
    };
    let mayor = entity(&mut app, 0).unwrap();
    let (here, there) = (housed(&app, mayor), housed(&app, miller));
    assert_ne!(here.town, there.town);
    // Each first mayor has its town's hall.
    let society = app.world().resource::<Society>();
    assert_eq!(society.town(there.town).unwrap().mayor, there.id);

    // A road between the mayors crosses the towns.
    let ends = if mayor <= miller {
        (mayor, miller)
    } else {
        (miller, mayor)
    };
    let traffic = Traffic {
        kind: protocol::RoadKind::Mayors,
        level: RoadUse::Dirt,
        messages: 0,
        last_from: None,
    };
    app.world_mut().spawn((Road { ends }, traffic));
    app.update();
    let pair = Pair::new(here.id, there.id);
    assert!(app.world().resource::<Society>().road(pair).is_some());

    // An agent that is gone for good leaves a ruin.
    let two = entity(&mut app, 2).unwrap();
    let was = app.world().get::<Housed>(two).unwrap().0;
    app.world_mut().entity_mut(two).despawn();
    let society = app.world().resource::<Society>();
    assert!(society.agent(was).unwrap().archived);
}

/// The place of each hamlet's town, by the hamlet's name.
fn towns(app: &mut App) -> Vec<(String, Vec2)> {
    let world = app.world_mut();
    let mut found = world
        .query::<(&HamletId, &Sited)>()
        .iter(world)
        .map(|(id, sited)| (id.0.clone(), sited.0))
        .collect::<Vec<_>>();
    found.sort();
    let (land, society) =
        (world.resource::<Land>(), world.resource::<Society>());
    found
        .into_iter()
        .map(|(name, town)| {
            let site = society.town(town).unwrap().site;
            (name, land.sites[site].at)
        })
        .collect()
}

/// Open ground far from every town that the town of `hamlet`
/// fits on.
fn meadow(app: &mut App, hamlet: Entity) -> Vec2 {
    let town = town_of(app.world(), hamlet).unwrap();
    let land = app.world().resource::<Land>();
    let society = app.world().resource::<Society>();
    let centre = land.hamlet_centre();
    (0..900)
        .map(|i| {
            centre
                + Vec2::from_angle(i as f32)
                    * (150.0 + i as f32 * 0.4)
        })
        .find(|&p| society.fits(land, Ground::Town(town), p).is_ok())
        .unwrap()
}

#[test]
fn towns_never_set_down_stand_on_the_first_free_sites() {
    let (mut app, _) = app();
    settle(&mut app, "mill");
    settle(&mut app, "forge");
    let world = app.world_mut();
    assert!(
        world
            .query_filtered::<&Spot, With<HamletId>>()
            .iter(world)
            .all(|spot| spot.0.is_none())
    );
    app.update();
    let land = app.world().resource::<Land>().clone();
    let fresh = testing::land(1);
    assert_eq!(land.sites, fresh.sites);
    let free = fresh
        .sites
        .iter()
        .filter(|site| site.buildable)
        .map(|site| site.at)
        .collect::<Vec<_>>();
    let mut stand = towns(&mut app)
        .into_iter()
        .map(|(_, at)| at)
        .collect::<Vec<_>>();
    stand.sort_by(|a, b| a.x.total_cmp(&b.x));
    let mut first = free[..3].to_vec();
    first.sort_by(|a, b| a.x.total_cmp(&b.x));
    assert_eq!(stand, first);
}

#[test]
fn a_town_follows_where_the_record_sets_it_down() {
    let (mut app, hamlet) = app();
    let mill = settle(&mut app, "mill");
    tell(&mut app, hamlet, &[1, 2], &[]);
    let before = towns(&mut app);
    let people = app.world().resource::<Society>().clone();
    let to = meadow(&mut app, mill);
    let view = *app.world().resource::<GeneratorView>();
    // The record keeps places from the hamlet island's middle.
    let centre = app.world().resource::<Land>().hamlet_centre();
    let said = to - centre;

    app.world_mut()
        .entity_mut(mill)
        .insert(Spot(Some((said.x, said.y))));
    app.update();
    let after = towns(&mut app);
    assert_eq!(after[0], before[0]);
    assert_eq!(after[1], ("mill".to_string(), to));
    // Everyone is who and where in town they were.
    assert_eq!(*app.world().resource::<Society>(), people);
    // The camera looks where it looked, though the middle of
    // the hamlets is elsewhere now.
    let now = *app.world().resource::<GeneratorView>();
    assert!(now.focus.distance(view.focus) < 0.01);
    assert!((now.dist - view.dist).abs() < 0.01);
    assert_ne!(app.world().resource::<Centre>().0, view.focus);

    // Ground that cannot take it leaves it where it is.
    let home = before[0].1 - centre;
    app.world_mut()
        .entity_mut(mill)
        .insert(Spot(Some((home.x + 6.0, home.y))));
    app.update();
    assert_eq!(towns(&mut app), after);

    // A world opened later puts it where the record says from
    // the start, and the others where they were.
    let mut later = bare();
    settle(&mut later, HOME);
    let mill = settle(&mut later, "mill");
    later
        .world_mut()
        .entity_mut(mill)
        .insert(Spot(Some((said.x, said.y))));
    later.update();
    assert_eq!(towns(&mut later), after);
}

#[test]
fn a_received_realm_houses_the_founder_in_its_hall() {
    let mut app = bare();
    for line in
        include_str!("../../../hamlet_protocol/frames/society.jsonl")
            .lines()
    {
        let frame =
            serde_json::from_str::<serde_json::Value>(line).unwrap();
        if frame["from"] == "server" {
            receive(
                app.world_mut(),
                serde_json::from_value(frame["message"].clone())
                    .unwrap(),
            );
        }
    }
    app.update();
    let world = app.world_mut();
    let ada =
        hamlet_client::realm::agent(world, protocol::AgentId(2))
            .unwrap();
    let housed = world.get::<Housed>(ada).unwrap().0;
    let society = world.resource::<Society>();
    let founder = society.agent(housed).unwrap();
    assert_eq!(society.towns.len(), 1);
    assert_eq!(society.agents.len(), 3);
    assert_eq!(society.towns[0].mayor, founder.id);
    assert_eq!(founder.plot, 0);
    assert_eq!(founder.state, KeeperState::Rest);
}

#[test]
fn a_frame_from_the_server_houses_the_founder_in_the_update_it_arrives()
 {
    use std::time::Duration;

    use hamlet_client::Tokio;
    use hamlet_client::connection::{Server, Wake};
    use hamlet_protocol::client::discovery::ServerAddress;

    let runtime = tokio::runtime::Runtime::new().unwrap();
    let url = realm::serve(
        runtime.handle(),
        vec![protocol::ServerMessage::State(realm::state())],
    );
    let (woke, wakes) = std::sync::mpsc::channel();
    let mut app = bare();
    app.insert_resource(Server(ServerAddress {
        url,
        token: "frame-token".into(),
    }))
    .insert_resource(Tokio(runtime.handle().clone()))
    .insert_resource(Wake::new(move || {
        let _ = woke.send(());
    }));
    app.update();
    // One wake for the connection, one for the state.
    let wait = Duration::from_secs(5);
    wakes.recv_timeout(wait).unwrap();
    wakes.recv_timeout(wait).unwrap();
    app.update();
    let founder = agent(&mut app, 0);
    assert_eq!(app.world().resource::<Society>().towns.len(), 1);
    assert_eq!(founder.plot, 0);
}

#[test]
fn a_hire_housed_after_the_first_state_crosses_by_boat() {
    use bevy::transform::TransformPlugin;

    use super::crossing::Voyage;
    use crate::harbor::ferry::Ferry;
    use crate::pick::Following;

    let mut app = App::new();
    testing::rehearse(&mut app);
    app.add_plugins((
        TransformPlugin,
        ClientPlugin { server: None },
        crate::harbor::plugin,
        super::plugin,
    ))
    .init_resource::<Society>()
    .init_resource::<CameraSettings>()
    .init_resource::<Glide>()
    .init_resource::<Orbit>()
    .init_resource::<Leash>()
    .init_resource::<Centre>()
    .init_resource::<Stance>()
    .init_resource::<GeneratorView>()
    .init_resource::<Ahead>()
    .init_resource::<Following>()
    .init_resource::<Objecting>()
    .init_resource::<crate::sky::Quality>()
    .init_resource::<crate::pick::PickSettings>();
    let hamlet = settle(&mut app, HOME);
    app.update();
    let voyages = |app: &mut App| {
        let world = app.world_mut();
        world.query::<&Voyage>().iter(world).count()
    };
    // The founder was housed in the first state.
    assert_eq!(voyages(&mut app), 0);

    tell(&mut app, hamlet, &[1], &[]);
    assert_eq!(voyages(&mut app), 1);

    // Past its walk to the pier, it is aboard a ferry.
    app.world_mut().resource_mut::<Clock>().t = 1000.0;
    app.update();
    let world = app.world_mut();
    assert_eq!(world.query::<&Ferry>().iter(world).count(), 1);
}
