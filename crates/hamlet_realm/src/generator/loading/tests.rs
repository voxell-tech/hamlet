use std::collections::HashSet;
use std::time::{Duration, Instant};

use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::window::RequestRedraw;

use super::lanes::Lanes;
use super::stream::stream;
use super::{Ground, Loading, Standing};
use crate::WorldCamera;
use crate::generator::ground::{Tile, rim_key};
use crate::generator::land::Land;
use crate::generator::{
    Ahead, DetailSettings, GenView, Tally, testing,
};
use crate::sky::CameraSettings;

/// The ground alone, seen by a camera with the world's fog.
fn app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<StandardMaterial>>()
        .add_message::<RequestRedraw>()
        .init_resource::<Loading>()
        .init_resource::<Lanes>()
        .init_resource::<Tally>()
        .init_resource::<DetailSettings>()
        .init_resource::<CameraSettings>()
        .init_resource::<Ahead>()
        .insert_resource(testing::land(7))
        .add_systems(Update, stream);
    testing::kit(app.world_mut());
    app.world_mut().spawn((
        WorldCamera,
        Transform::default(),
        Projection::Perspective(PerspectiveProjection {
            far: 4000.0,
            ..default()
        }),
        DistanceFog::default(),
    ));
    app
}

/// Looks at `focus` from `far` away, as the world's camera
/// does, and runs frames until the ground is in.
fn look(app: &mut App, focus: Vec2, far: f32) {
    let world = app.world_mut();
    let mut camera = world
        .query_filtered::<(&mut Transform, &mut DistanceFog), With<WorldCamera>>();
    let (mut at, mut fog) = camera.single_mut(world).unwrap();
    let focus = Vec3::new(focus.x, 1.3, focus.y);
    *at = Transform::from_translation(
        focus + far * Vec3::new(0.0, 0.64, 0.77),
    )
    .looking_at(focus, Vec3::Y);
    fog.falloff = FogFalloff::Linear {
        start: far * 1.2,
        end: far * 2.6 + 150.0,
    };
    let began = Instant::now();
    loop {
        app.update();
        let loading = app.world().resource::<Loading>();
        if loading.done() && loading.tiles().1 == 0 {
            return;
        }
        assert!(began.elapsed() < Duration::from_secs(120));
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// The tiles drawn, and the trees that stand.
fn drawn(app: &mut App) -> (Vec<Tile>, usize) {
    let world = app.world_mut();
    let tiles = world
        .query::<&Ground>()
        .iter(world)
        .map(|ground| ground.0)
        .collect::<Vec<_>>();
    let trees = world
        .query_filtered::<(), With<Standing>>()
        .iter(world)
        .count();
    (tiles, trees)
}

#[test]
fn the_ground_follows_the_view_and_what_is_left_behind_goes() {
    let mut app = app();
    let chunk = 32.0;
    let land = app.world().resource::<Land>().clone();
    let mut most = (0, 0);
    for (focus, far) in [
        (Vec2::ZERO, 72.0),
        (Vec2::new(400.0, 90.0), 72.0),
        (Vec2::new(9000.0, -7000.0), 300.0),
        (Vec2::new(-60_000.0, 800.0), 30.0),
        (Vec2::ZERO, 72.0),
    ] {
        look(&mut app, focus, far);
        let (tiles, trees) = drawn(&mut app);
        most = (most.0.max(tiles.len()), most.1.max(trees));
        // The tiles drawn are the ones this view calls for, whole and
        // with none over another.
        let reach = (far * 2.6 + 150.0) * 0.9;
        for (i, a) in tiles.iter().enumerate() {
            assert!(a.middle(chunk).distance(focus) < reach * 2.0);
            for b in &tiles[i + 1..] {
                assert!(!a.covers(*b) && !b.covers(*a));
            }
        }
        let under = Tile {
            level: 0,
            x: (focus.x / chunk).floor() as i32,
            z: (focus.y / chunk).floor() as i32,
        };
        assert!(tiles.iter().any(|tile| tile.covers(under)));
    }
    // The cost is bounded by the view, wherever it went.
    assert!(most.0 < 1500, "{} tiles", most.0);
    assert!(most.1 < 30_000, "{} trees", most.1);
    // Looking about founded nothing and moved nothing.
    assert_eq!(app.world().resource::<Land>().sites, land.sites);

    // Back where it began, the ground is what it was at first.
    let again = drawn(&mut app);
    let mut fresh = self::app();
    look(&mut fresh, Vec2::ZERO, 72.0);
    let first = drawn(&mut fresh);
    let sorted = |mut tiles: Vec<Tile>| {
        tiles.sort_by_key(|t| (t.level, t.x, t.z));
        tiles
    };
    assert_eq!(sorted(again.0), sorted(first.0));
    assert_eq!(again.1, first.1);
}

#[test]
fn the_ground_is_there_before_a_camera_that_is_on_its_way() {
    let mut app = app();
    look(&mut app, Vec2::ZERO, 72.0);
    let here = drawn(&mut app).0;
    // Sent somewhere far, the camera has not moved yet.
    let there = Vec2::new(2600.0, -900.0);
    app.world_mut().resource_mut::<Ahead>().0 = Some(GenView {
        focus: there,
        dist: 72.0,
        yaw: 0.0,
        pitch: 40.0,
    });
    look(&mut app, Vec2::ZERO, 72.0);
    let both = drawn(&mut app).0;
    let under = |at: Vec2| Tile {
        level: 0,
        x: (at.x / 32.0).floor() as i32,
        z: (at.y / 32.0).floor() as i32,
    };
    assert!(both.iter().any(|tile| tile.covers(under(there))));
    assert!(here.iter().all(|tile| both.contains(tile)));
    for (i, a) in both.iter().enumerate() {
        for b in &both[i + 1..] {
            assert!(!a.covers(*b) && !b.covers(*a));
        }
    }
    // Come there, it finds the ground as it will stay.
    app.world_mut().resource_mut::<Ahead>().0 = None;
    let before = drawn(&mut app).0.len();
    look(&mut app, there, 72.0);
    let after = drawn(&mut app).0;
    assert!(after.len() < before);
    assert!(after.iter().all(|tile| both.contains(tile)));

    // Asked to stand further back from the same place, the
    // larger tiles lie over none of those that are drawn.
    app.world_mut().resource_mut::<Ahead>().0 = Some(GenView {
        focus: there,
        dist: 500.0,
        yaw: 0.0,
        pitch: 40.0,
    });
    look(&mut app, there, 72.0);
    let both = drawn(&mut app).0;
    assert!(both.iter().any(|tile| tile.level >= 3));
    for (i, a) in both.iter().enumerate() {
        for b in &both[i + 1..] {
            assert!(!a.covers(*b) && !b.covers(*a));
        }
    }
}

#[test]
fn ground_a_town_comes_to_or_leaves_is_built_anew() {
    let mut app = app();
    look(&mut app, Vec2::ZERO, 72.0);
    let entities = |app: &mut App| {
        let world = app.world_mut();
        world
            .query::<(Entity, &Ground)>()
            .iter(world)
            .map(|(entity, ground)| (ground.0, entity))
            .collect::<HashMap<_, _>>()
    };
    let before = entities(&mut app);
    let (from, to) = {
        let mut land = app.world_mut().resource_mut::<Land>();
        let from = land.sites[0].at;
        let to = (0..600)
            .map(|i| Vec2::from_angle(i as f32) * (60.0 + i as f32))
            .find(|&p| {
                land.suits(p).is_ok()
                    && land
                        .sites
                        .iter()
                        .all(|s| s.at.distance(p) > 80.0)
            })
            .unwrap();
        land.move_site(0, to);
        (from, to)
    };
    look(&mut app, Vec2::ZERO, 72.0);
    let after = entities(&mut app);
    let anew = |tile: &Tile| before.get(tile) != after.get(tile);
    let near = |tile: &Tile, at: Vec2| {
        tile.middle(32.0).distance(at) < tile.side(32.0) * 0.5
    };
    assert!(after.keys().any(|t| near(t, from) && anew(t)));
    assert!(after.keys().any(|t| near(t, to) && anew(t)));
    // Ground far from both, beside tiles of the sizes they were, is
    // the ground that was there.
    let top = DetailSettings::default().tile_levels - 1;
    let tiles = |drawn: &HashMap<Tile, Entity>| {
        drawn.keys().copied().collect::<HashSet<_>>()
    };
    let (was, is) = (tiles(&before), tiles(&after));
    let kept = after
        .keys()
        .filter(|t| {
            let mid = t.middle(32.0);
            before.contains_key(*t)
                && mid.distance(from) > t.side(32.0) + 60.0
                && mid.distance(to) > t.side(32.0) + 60.0
                && rim_key(&was, top, **t) == rim_key(&is, top, **t)
        })
        .collect::<Vec<_>>();
    assert!(kept.len() > 50);
    assert!(kept.into_iter().all(|t| !anew(t)));
}
