//! The bazaar's place in the world: where it is pitched, and that it
//! keeps the towns off its ground.

use super::*;
use crate::chief::ChiefSettings;
use crate::generator::land::Land;
use crate::generator::testing::rehearse;
use crate::palette::Palette;
use crate::pick::Selected;

/// A world with the first town founded and the bazaar's systems,
/// with nothing drawn and no chief to wait for.
fn app() -> App {
    let mut app = App::new();
    rehearse(&mut app);
    app.init_resource::<Society>()
        .init_resource::<Palette>()
        .init_resource::<ChiefSettings>()
        .init_resource::<Selected>()
        .add_plugins(plugin);
    found(&mut app);
    app
}

fn found(app: &mut App) {
    app.world_mut()
        .resource_scope(|world, mut land: Mut<Land>| {
            world.resource_mut::<Society>().found_town(&mut land);
        });
}

fn bazaars(app: &mut App) -> Vec<Vec2> {
    let world = app.world_mut();
    world
        .query::<&Bazaar>()
        .iter(world)
        .map(|bazaar| bazaar.held)
        .collect()
}

#[test]
fn towns_founded_after_the_bazaar_keep_off_its_ground() {
    let mut app = app();
    app.update();
    let [at] = bazaars(&mut app)[..] else {
        panic!("one bazaar");
    };
    let world = app.world();
    let (land, society) =
        (world.resource::<Land>(), world.resource::<Society>());
    let first = land.sites[society.towns[0].site].at;
    assert!(at.distance(first) > land.set.land.site_spacing * 0.5);
    let (_, radius) = society.stands(land, Ground::Bazaar).unwrap();
    assert!(radius >= GROUNDS);

    for _ in 0..6 {
        found(&mut app);
    }
    let world = app.world();
    let (land, society) =
        (world.resource::<Land>(), world.resource::<Society>());
    let room = land.set.town.keep_apart * 0.5;
    for town in &society.towns[1..] {
        let site = land.sites[town.site].at;
        assert!(
            site.distance(at) >= room + radius - 1e-3,
            "a town at {site} on the bazaar's ground at {at}"
        );
    }
}

#[test]
fn a_bazaar_set_down_elsewhere_is_pitched_there_alone() {
    let mut app = app();
    app.update();
    let [at] = bazaars(&mut app)[..] else {
        panic!("one bazaar");
    };
    let to = at + Vec2::new(-22.0, 10.0);
    app.world_mut().resource_mut::<Pitched>().0 = Some(to);
    app.update();
    assert_eq!(bazaars(&mut app), [to]);
    let society = app.world().resource::<Society>();
    let claims = society
        .claims
        .iter()
        .filter(|claim| claim.place == Ground::Bazaar)
        .map(|claim| claim.at)
        .collect::<Vec<_>>();
    assert_eq!(claims, [to]);
}
