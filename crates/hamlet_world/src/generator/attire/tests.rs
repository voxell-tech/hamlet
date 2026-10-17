use std::time::{Duration, Instant};

use bevy::gltf::{Gltf, GltfMesh, GltfNode};
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
use bevy::render::RenderPlugin;
use bevy::render::settings::WgpuSettings;
use bevy::state::app::StatesPlugin;
use bevy::window::{ExitCondition, RequestRedraw};
use bevy::winit::WinitPlugin;
use hamlet_model::wardrobe::{
    Emote, Fireflies, Lanterns, Petals, Pinwheels, Skin, Target,
};

use super::{
    Attire, AttireSettings, Attires, Firefly, Petal, Pinwheel, Shell,
    Swing, skinned,
};
use crate::dress::{self, Closet, Glb, SOURCE};
use crate::generator::house::{self, Fit, TownStyle};
use crate::generator::land::Land;
use crate::generator::loading::{Loading, finish_loading};
use crate::generator::society::{AgentId, Society, TownId};
use crate::generator::sync::{Drawn, sync};
use crate::generator::{
    DetailSettings, GenOptions, HouseMotion, Phase, Tally, testing,
};
use crate::meshes::Mb;
use crate::sky::SkySettings;
use crate::{Clock, Sky};

fn skin(json: &str) -> Skin {
    Skin::read(json.as_bytes(), Target::Hamlet).unwrap()
}

fn colours(mesh: &Mesh) -> Vec<[f32; 4]> {
    match mesh.attribute(Mesh::ATTRIBUTE_COLOR) {
        Some(VertexAttributeValues::Float32x4(c)) => c.clone(),
        _ => panic!("a mesh with vertex colours"),
    }
}

/// True for a colour of nothing but blue.
fn blue(c: &[f32; 4]) -> bool {
    c[0] == 0.0 && c[1] == 0.0 && c[2] > 0.0
}

/// True for a colour of nothing but red.
fn red(c: &[f32; 4]) -> bool {
    c[0] > 0.0 && c[1] == 0.0 && c[2] == 0.0
}

#[test]
fn a_skin_recolours_its_slots_of_a_house_near_and_far_and_no_other() {
    let land = testing::land(7);
    let worn = skinned(
        &land.pal,
        &skin(
            r##"{"format":1,"colours":{"livery":"#0000ff",
            "roof":"#ff0000","drift":"#ff0000"}}"##,
        ),
    );
    let agent = AgentId(2);
    let yard = house::yard_of(&land, agent, false, 0.0, 0);
    let near = |pal| {
        let style = TownStyle::on(pal, land.seed, 0);
        let built = house::dressed(
            &land,
            pal,
            agent,
            &style,
            false,
            false,
            0,
            &yard,
            &Fit::default(),
        );
        colours(&built.body.build())
    };
    let (plain, dyed) = (near(&land.pal), near(&worn));
    assert_eq!(plain.len(), dyed.len());
    // The doors and shutters wear the livery, the roof its own
    // colour, and the walls are as they were.
    assert!(dyed.iter().any(blue) && !plain.iter().any(blue));
    let kept = plain
        .iter()
        .zip(&dyed)
        .filter(|(was, now)| was == now)
        .count();
    assert!(kept > dyed.len() / 2);

    let far = |pal| {
        let (mut mb, mut panes) = (Mb::new(), Mb::new());
        let style = TownStyle::on(pal, land.seed, 0);
        house::far(
            &mut mb,
            &mut panes,
            Default::default(),
            &land,
            pal,
            agent,
            &style,
            false,
            None,
            None,
        );
        colours(&mb.build())
    };
    let (plain, dyed) = (far(&land.pal), far(&worn));
    assert!(dyed.iter().any(red) && !plain.iter().any(red));
    for (was, now) in plain.iter().zip(&dyed) {
        assert!(was == now || red(now));
    }
}

/// The generator with nothing drawn and no ground: a town of four
/// houses, all there from the start.
fn town() -> (App, TownId) {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, StatesPlugin))
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<StandardMaterial>>()
        .add_message::<RequestRedraw>()
        .init_resource::<Clock>()
        .init_resource::<Sky>()
        .init_resource::<Drawn>()
        .init_resource::<Loading>()
        .init_resource::<Tally>()
        .init_resource::<GenOptions>()
        .init_resource::<SkySettings>()
        .init_resource::<DetailSettings>()
        .init_resource::<HouseMotion>()
        .init_state::<Phase>()
        .insert_resource(testing::land(7))
        .add_plugins(super::plugin)
        .add_systems(
            Update,
            (
                finish_loading,
                sync.run_if(
                    resource_changed::<Society>
                        .or_else(resource_changed::<Attires>),
                ),
            )
                .chain(),
        );
    testing::kit(app.world_mut());
    let mut land = app.world().resource::<Land>().clone();
    let mut society = Society::default();
    let town = society.found_town(&mut land);
    for _ in 0..3 {
        society.add_agent(&land, town).unwrap();
    }
    app.insert_resource(society);
    settle(&mut app);
    (app, town)
}

fn settle(app: &mut App) {
    let began = Instant::now();
    loop {
        app.update();
        if app.world().resource::<Loading>().builds.is_empty() {
            return;
        }
        assert!(began.elapsed() < Duration::from_secs(60));
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Each house drawn, by agent, and the colours of its body.
fn houses(app: &App) -> Vec<(AgentId, Entity, Vec<[f32; 4]>)> {
    let world = app.world();
    let mut all = world
        .resource::<Drawn>()
        .houses
        .iter()
        .map(|(agent, (entity, ..))| {
            let mesh = world.get::<Mesh3d>(*entity).unwrap();
            let mesh =
                world.resource::<Assets<Mesh>>().get(mesh).unwrap();
            (*agent, *entity, colours(mesh))
        })
        .collect::<Vec<_>>();
    all.sort_by_key(|(agent, ..)| agent.0);
    all
}

/// The lanterns hung from `house`.
fn lanterns(app: &App, house: Entity) -> usize {
    let world = app.world();
    world
        .get::<Children>(house)
        .into_iter()
        .flatten()
        .filter(|child| world.get::<Swing>(**child).is_some())
        .count()
}

#[test]
fn a_town_is_built_anew_in_what_its_hamlet_wears_and_again_without() {
    let (mut app, town) = town();
    let before = houses(&app);
    assert_eq!(before.len(), 4);
    let attire = Attire {
        skin: Some(skin(
            r##"{"format":1,"colours":{"livery":"#0000ff"}}"##,
        )),
        mesh: None,
        emote: Some(Emote::Lanterns(Lanterns {
            paper: "#ffffff".into(),
            light: "#ffaa00".into(),
            count: 3,
            sway: 10.0,
        })),
    };
    app.world_mut()
        .resource_mut::<Attires>()
        .wear(town, attire.clone());
    settle(&mut app);
    let worn = houses(&app);
    for ((_, was, _), (_, now, colours)) in before.iter().zip(&worn) {
        assert_ne!(was, now);
        assert!(colours.iter().any(blue));
        assert_eq!(lanterns(&app, *now), 3);
    }
    // A house raised on its own wears it the same.
    let raised =
        super::raise(app.world_mut(), &attire, false, default());
    assert_eq!(lanterns(&app, raised), 3);
    app.world_mut().entity_mut(raised).despawn();

    app.world_mut()
        .resource_mut::<Attires>()
        .wear(town, Attire::default());
    settle(&mut app);
    let after = houses(&app);
    for ((_, _, was), (_, now, colours)) in before.iter().zip(&after)
    {
        assert_eq!(was, colours);
        assert_eq!(lanterns(&app, *now), 0);
    }
    let world = app.world_mut();
    assert_eq!(world.query::<&Swing>().iter(world).count(), 0);
}

#[test]
fn a_worn_mesh_stands_for_the_walls_and_its_spots_are_used() {
    let land = testing::land(7);
    let plain = house::plain(&land);
    let lamp = Vec3::new(0.4, 1.3, 1.25);
    let chimney = Vec3::new(-0.5, 3.1, -0.2);
    let at = Transform::from_translation;
    let bytes = Glb::new()
        .mesh("walls", &plain.walls, Transform::IDENTITY)
        .mesh("panes", &plain.panes, Transform::IDENTITY)
        .spot("door", at(plain.spots.door))
        .spot("lamp", at(lamp))
        .spot("chimney", at(chimney))
        .write();

    let mut app = App::new();
    dress::register_source(&mut app);
    app.add_plugins(
        DefaultPlugins
            .build()
            .disable::<WinitPlugin>()
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
            .set(RenderPlugin {
                render_creation: WgpuSettings {
                    backends: None,
                    ..default()
                }
                .into(),
                ..default()
            }),
    );
    app.finish();
    app.cleanup();
    app.world()
        .resource::<Closet>()
        .keep(std::path::Path::new("worn.glb"), bytes);
    let gltf = app
        .world()
        .resource::<AssetServer>()
        .load::<Gltf>(format!("{SOURCE}://worn.glb"));
    let shell = (0..500)
        .find_map(|_| {
            app.update();
            std::thread::sleep(Duration::from_millis(5));
            let world = app.world();
            Shell::of(
                &gltf,
                world.resource::<Assets<Gltf>>(),
                world.resource::<Assets<GltfMesh>>(),
                world.resource::<Assets<GltfNode>>(),
                world.resource::<Assets<Mesh>>(),
            )
        })
        .expect("the mesh to load");
    assert!(shell.panes.is_some());
    assert_eq!(shell.spots.lamp, lamp);
    // Not marked, the eave is found from the walls and the lamp.
    assert_eq!(shell.spots.eaves.z, shell.bulk.max.z);

    let agent = AgentId(2);
    let yard = house::yard_of(&land, agent, false, 0.0, 0);
    let style = TownStyle::of(&land, 0);
    let build = |fit| {
        house::dressed(
            &land, &land.pal, agent, &style, false, false, 0, &yard,
            &fit,
        )
    };
    let own = build(Fit::default());
    let worn = build(
        Attire {
            mesh: Some(shell),
            ..default()
        }
        .fit(),
    );
    let by = worn.worn.expect("a worn mesh's scale");
    assert_eq!(by.y, 1.0);
    assert!(by.x > 1.0 && by.z > 1.0);
    assert_eq!(worn.spots.lamp, lamp * by);
    assert_eq!(worn.spots.chimney, chimney * by);
    // Its own walls, roof and windows are left out, its grounds not.
    assert!(worn.panes.is_empty());
    assert!(worn.body.tris() * 2 < own.body.tris());
    assert!(!worn.body.is_empty());
}

/// How many children of `house` carry `C`.
fn count<C: Component>(app: &App, house: Entity) -> usize {
    let world = app.world();
    world
        .get::<Children>(house)
        .into_iter()
        .flatten()
        .filter(|child| world.get::<C>(**child).is_some())
        .count()
}

#[test]
fn petals_fireflies_and_pinwheels_hang_about_a_house_as_many_as_allowed()
 {
    let (mut app, _) = town();
    let set = app.world().resource::<AttireSettings>().clone();
    let raise = |app: &mut App, emote: Emote| {
        let attire = Attire {
            emote: Some(emote),
            ..Attire::default()
        };
        super::raise(app.world_mut(), &attire, false, default())
    };
    let house = raise(
        &mut app,
        Emote::Petals(Petals {
            colours: ["#ffc0cb".into(), "#ffffff".into()],
            count: 4,
            size: 0.06,
            fall: 6.0,
        }),
    );
    assert_eq!(count::<Petal>(&app, house), 4);
    // Asking for more than the settings allow gets no more.
    let house = raise(
        &mut app,
        Emote::Fireflies(Fireflies {
            colour: "#eeff88".into(),
            count: 1000,
            glow: 4.0,
        }),
    );
    assert_eq!(count::<Firefly>(&app, house), set.fireflies as usize);
    let house = raise(
        &mut app,
        Emote::Pinwheels(Pinwheels {
            colours: ["#ff0000".into(), "#ffff00".into()],
            count: 2,
            speed: 1.0,
        }),
    );
    assert_eq!(count::<Pinwheel>(&app, house), 2);
}

#[test]
fn a_petal_falls_from_its_eave_to_the_ground_and_comes_round_again() {
    let petal = Petal {
        from: Vec3::new(0.0, 2.0, 1.0),
        size: 0.1,
        life: 4.0,
        phase: 0.0,
        swirl: 0.3,
    };
    let high = |t: f32| petal.at(t).translation.y;
    assert!((high(0.0) - 2.0).abs() < 1e-4);
    assert!(high(1.0) > high(2.0) && high(2.0) > high(3.0));
    assert!(high(3.999) < 0.1 && high(3.999) > 0.0);
    // Gone at both ends of its fall: it never pops in or out.
    assert!(petal.at(0.0).scale.x < 1e-4);
    assert!(petal.at(3.9999).scale.x < 1e-3);
    assert!((high(4.0) - high(0.0)).abs() < 1e-3);
}

#[test]
fn a_firefly_wanders_about_its_house_and_never_into_it() {
    let fly = Firefly {
        near: 1.5,
        reach: 0.6,
        high: 1.0,
        speed: 0.3,
        phase: 0.4,
    };
    let mut sides = [false; 4];
    for step in 0..2000 {
        let at = fly.at(step as f32 * 0.05).translation;
        let out = Vec2::new(at.x, at.z).length();
        assert!((1.5 - 1e-3..=2.1 + 1e-3).contains(&out), "{out}");
        sides
            [usize::from(at.x > 0.0) * 2 + usize::from(at.z > 0.0)] =
            true;
    }
    // It goes all the way round.
    assert_eq!(sides, [true; 4]);
}
