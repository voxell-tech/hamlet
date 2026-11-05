use super::*;
use crate::generator::testing::rehearse;
use crate::pick::{Glide, Leash};
use crate::sky::CameraSettings;

fn harbor() -> App {
    let mut app = App::new();
    rehearse(&mut app);
    app.add_plugins(plugin)
        .init_resource::<CameraSettings>()
        .init_resource::<Centre>()
        .init_resource::<Orbit>()
        .init_resource::<Glide>()
        .init_resource::<Following>()
        .init_resource::<Leash>();
    app
}

/// Where the pier of `isle` stands in the world.
fn stands(app: &mut App, isle: IslandId) -> Vec3 {
    let world = app.world_mut();
    world
        .query::<(&Pier, &Transform)>()
        .iter(world)
        .find(|(pier, _)| pier.0 == isle)
        .map(|(_, at)| at.translation)
        .expect("a pier")
}

#[test]
fn a_pier_stands_on_each_coast_facing_the_other_island() {
    let mut app = harbor();
    app.update();
    let land = app.world().resource::<Land>().clone();
    for isle in IslandId::BOTH {
        let at = stands(&mut app, isle);
        let coast = at.xz();
        assert_eq!(land.island_at(coast), isle);
        assert!(!land.is_water(coast), "{isle:?}");
        let out = land.toward_other(isle);
        assert!(land.is_water(coast + out * 4.0), "{isle:?}");
        // The boat lies at it.
        let world = app.world_mut();
        let boats = world
            .query::<&Boat>()
            .iter(world)
            .filter(|boat| boat.0 == isle)
            .count();
        assert_eq!(boats, 1);
    }
    // And all the sea between them is open.
    let [a, b] =
        IslandId::BOTH.map(|isle| stands(&mut app, isle).xz());
    for k in 1..20 {
        let p = a.lerp(b, k as f32 / 20.0);
        assert!(land.is_water(p), "{p}");
    }
}

#[test]
fn the_pier_moves_when_its_island_grows() {
    let mut app = harbor();
    app.update();
    let before = stands(&mut app, IslandId::City);
    let toward = app.world().resource::<Land>().bearing();
    let out = toward * 330.0;
    app.world_mut().resource_mut::<Land>().fill_city(&[out]);
    app.update();
    let after = stands(&mut app, IslandId::City);
    assert!(
        after.xz().distance(Vec2::ZERO)
            > before.xz().distance(Vec2::ZERO) + 100.0,
        "{before} -> {after}"
    );
    let land = app.world().resource::<Land>();
    assert!(!land.is_water(after.xz()));
    // Only one set of piers stands.
    let world = app.world_mut();
    assert_eq!(world.query::<&Pier>().iter(world).count(), 2);
}

#[test]
fn the_harbor_control_glides_to_the_other_island() {
    let mut app = harbor();
    app.update();
    let land = app.world().resource::<Land>().clone();
    app.world_mut().resource_mut::<Leash>().anchors =
        land.shores().collect();
    for start in IslandId::BOTH {
        let from = land.island(start).centre;
        let centre = app.world().resource::<Centre>().0;
        app.world_mut().resource_mut::<Orbit>().pan = from - centre;
        glide_to_other_pier(app.world_mut());
        let at = centre + app.world().resource::<Orbit>().pan;
        assert_eq!(land.island_at(at), start.other());
        let pier = stands(&mut app, start.other()).xz();
        assert!(at.distance(pier) < shapes::SEAWARD, "{at} {pier}");
    }
}
