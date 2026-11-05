use bevy::transform::TransformPlugin;
use hamlet_client::ClientPlugin;
use hamlet_client::realm::{Identity, Scholar};
use hamlet_protocol::client as protocol;

use super::houses::Home;
use super::newcomers::Newcomer;
use super::*;
use crate::generator::society::Society;
use crate::generator::testing::rehearse;
use crate::harbor::Harbor;
use crate::pick::PickSettings;
use crate::sky::Quality;

/// The way every house's door faces.
const TURN: f64 = 1.0;

fn scholar(
    id: u64,
    house: Option<(f64, f64)>,
    hires: Vec<u64>,
    left: bool,
) -> Scholar {
    Scholar(protocol::Scholar {
        arrival: if house.is_some() {
            protocol::Arrival::Housed
        } else {
            protocol::Arrival::Waiting
        },
        chief: String::new(),
        hires: hires.into_iter().map(protocol::AgentId).collect(),
        house: house.map(|(x, z)| protocol::HouseAt {
            turn: TURN,
            x,
            z,
        }),
        id: protocol::ScholarId(id),
        left: left.then(|| protocol::ScholarLeft {
            at: 0,
            by: String::new(),
            reason: String::new(),
        }),
        name: format!("scholar {id}"),
        since: 0,
        skills: Vec::new(),
        versions: Vec::new(),
    })
}

fn city() -> App {
    let mut app = App::new();
    rehearse(&mut app);
    app.add_plugins((
        TransformPlugin,
        ClientPlugin { server: None },
        crate::harbor::plugin,
        plugin,
    ))
    .init_resource::<Society>()
    .init_resource::<Quality>()
    .init_resource::<PickSettings>();
    app
}

#[test]
fn the_city_has_a_tower_and_a_house_for_each_scholar_with_a_spot() {
    let mut app = city();
    let world = app.world_mut();
    // One of its two hires lives.
    world.spawn(Identity(protocol::AgentId(11)));
    world.spawn((
        Name::new("a"),
        scholar(1, Some((60.0, 20.0)), vec![11, 12], false),
    ));
    // Without a spot it has no house, and one that left has its
    // windows shuttered whoever it hired.
    world.spawn((Name::new("b"), scholar(2, None, vec![], false)));
    world.spawn((
        Name::new("c"),
        scholar(3, Some((-60.0, 20.0)), vec![11], true),
    ));
    app.update();
    app.update();

    let world = app.world_mut();
    assert_eq!(world.query::<&Tower>().iter(world).count(), 1);
    let mut homes = world
        .query::<&Home>()
        .iter(world)
        .map(Home::windows)
        .collect::<Vec<_>>();
    homes.sort();
    assert_eq!(homes, [(0, true), (1, false)]);
    // Each is turned as its scholar's house says.
    let turns = world
        .query_filtered::<&Transform, With<Home>>()
        .iter(world)
        .map(|at| at.rotation.to_euler(EulerRot::YXZ).0)
        .collect::<Vec<_>>();
    assert_eq!(turns.len(), 2);
    for turn in turns {
        assert!((turn - TURN as f32).abs() < 1e-4, "{turn}");
    }
    // The island is grown about the tower and both houses.
    let land = world.resource::<Land>();
    for spot in [Vec2::ZERO, Vec2::new(60.0, 20.0)] {
        assert!(land.city_spots().contains(&spot), "{spot}");
    }
}

#[test]
fn a_waiting_scholar_stands_on_the_pier_until_it_has_a_house() {
    let mut app = city();
    let waiting = app
        .world_mut()
        .spawn((Name::new("a"), scholar(1, None, vec![], false)))
        .id();
    app.update();
    app.update();

    let on_pier = |app: &mut App| {
        let pier = app.world().resource::<Harbor>().spot(
            IslandId::City,
            8.0,
            0.0,
        );
        let world = app.world_mut();
        world
            .query_filtered::<&Transform, With<Newcomer>>()
            .iter(world)
            .filter(|at| at.translation.xz().distance(pier) < 10.0)
            .count()
    };
    assert_eq!(on_pier(&mut app), 1);
    let homes = |app: &mut App| {
        let world = app.world_mut();
        world.query::<&Home>().iter(world).count()
    };
    assert_eq!(homes(&mut app), 0);

    // Let in, it leaves the pier for its house.
    app.world_mut().entity_mut(waiting).insert(scholar(
        1,
        Some((60.0, 20.0)),
        vec![],
        false,
    ));
    app.update();
    app.update();
    assert_eq!(on_pier(&mut app), 0);
    assert_eq!(homes(&mut app), 1);
}
