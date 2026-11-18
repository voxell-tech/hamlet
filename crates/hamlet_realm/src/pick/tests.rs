use bevy::ecs::system::SystemState;

use super::*;
use crate::sky::CameraSettings;

#[test]
fn the_view_stays_within_reach_of_something_to_look_at() {
    let camera = CameraSettings::default();
    let centre = Vec2::new(100.0, 0.0);
    // With nothing to hold on to, the middle is what counts.
    let free = Leash::default();
    let far = free.held(&camera, centre, Vec2::X * 9000.0);
    assert_eq!(far, Vec2::X * camera.pan);
    // With towns, it is the nearest of them, however far from
    // the middle it lies.
    let towns = Leash {
        zoom: None,
        anchors: vec![Vec2::ZERO, Vec2::new(5000.0, 300.0)],
    };
    let by = Vec2::new(4900.0, 280.0) - centre;
    assert_eq!(towns.held(&camera, centre, by), by);
    let past = Vec2::new(5000.0, 300.0 + 2.0 * camera.pan) - centre;
    let held = towns.held(&camera, centre, past);
    let stands = centre + held;
    assert!(
        (stands.distance(towns.anchors[1]) - camera.pan).abs() < 0.01
    );
    // The way from one town to another is open all along, and
    // so far to its side.
    let between = Vec2::new(2500.0, 150.0) - centre;
    assert_eq!(towns.held(&camera, centre, between), between);
    let aside = Vec2::new(2500.0, 150.0 + 3.0 * camera.pan);
    let held = centre + towns.held(&camera, centre, aside - centre);
    assert!(held.distance(aside) > camera.pan);
    assert!(
        (held.distance(Vec2::new(2500.0, 150.0)) - camera.pan).abs()
            < 40.0
    );
}

#[test]
fn the_leads_name_names_its_things_and_a_profile_house_its_lodging() {
    use hamlet_client::realm::{AgentName, Identity, Lead, Lodging};
    use hamlet_protocol::client::AgentId;

    let mut world = World::new();
    let lead = world
        .spawn((Name::new("agent#2"), Lead, Identity(AgentId(2))))
        .id();
    let house = world
        .spawn((Name::new("ada"), Lodging("Countess".into())))
        .id();
    let bare = world.spawn(Name::new("bea")).id();
    let mut state = SystemState::<Names>::new(&mut world);
    let names = state.get(&world).unwrap();
    assert_eq!(Thing::Keeper.name(&names), "The Lead");
    assert_eq!(Thing::Cottage.sign(&names), "Lead");
    assert_eq!(Thing::Profile(house).sign(&names), "Countess");
    assert_eq!(Thing::Profile(bare).sign(&names), "bea");
    assert_eq!(
        Thing::Profile(house).name(&names),
        "Countess's House"
    );
    world
        .entity_mut(lead)
        .insert(AgentName("Chief of Staff".into()));
    let names = state.get(&world).unwrap();
    assert_eq!(Thing::Keeper.name(&names), "Chief of Staff");
    assert_eq!(Thing::Cottage.name(&names), "Chief of Staff's House");
    assert_eq!(Thing::Cottage.sign(&names), "Chief of Staff");
}
