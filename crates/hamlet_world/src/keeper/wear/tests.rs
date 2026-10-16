use std::collections::BTreeMap;
use std::path::Path;

use bevy::asset::io::memory::{Dir, MemoryAssetReader};
use bevy::asset::io::{AssetSourceBuilder, AssetSourceId};
use bevy::ecs::system::RunSystemOnce;
use bevy::render::RenderPlugin;
use bevy::render::settings::WgpuSettings;
use bevy::window::{ExitCondition, RequestRedraw};
use bevy::winit::WinitPlugin;
use hamlet_model::wardrobe::{Finish, Key, Move};

use super::*;
use crate::dress::Glb;
use crate::generator::DetailSettings;
use crate::keeper::{Who, default_parts, spawn_agent};
use crate::meshes::Mb;

fn robot(app: &mut App, who: Who, x: f32) -> Entity {
    app.world_mut()
        .run_system_once(
            move |mut commands: Commands,
                  kit: Res<Kit>,
                  mut materials: ResMut<Assets<StandardMaterial>>| {
                spawn_agent(
                    &mut commands,
                    &kit,
                    &mut materials,
                    who,
                    Transform::from_xyz(x, 0.0, 0.0),
                    None,
                )
            },
        )
        .unwrap()
}

/// A world of robots without a window, as the keeper's tests have.
fn app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, plugin))
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<StandardMaterial>>()
        .add_message::<RequestRedraw>()
        .init_resource::<Clock>()
        .init_resource::<crate::Stage>()
        .init_resource::<crate::Sky>()
        .init_resource::<DetailSettings>()
        .init_resource::<Motion>()
        .add_systems(Update, animate);
    crate::stock(app.world_mut());
    app
}

/// Each mesh of `part` of a robot: its mesh, its material and whether
/// it is shown.
fn drawn(
    app: &App,
    robot: Entity,
    part: Part,
) -> Vec<(Handle<Mesh>, Handle<StandardMaterial>, bool)> {
    let world = app.world();
    world
        .get::<Keeper>(robot)
        .unwrap()
        .parts
        .shapes
        .iter()
        .filter(|shape| shape.part == part)
        .map(|shape| {
            let e = world.entity(shape.entity);
            (
                e.get::<Mesh3d>().unwrap().0.clone(),
                e.get::<MeshMaterial3d<StandardMaterial>>()
                    .unwrap()
                    .0
                    .clone(),
                e.get::<Visibility>() != Some(&Visibility::Hidden),
            )
        })
        .collect()
}

fn skin() -> Skin {
    Skin {
        format: 1,
        colours: BTreeMap::from([
            ("paint".into(), "#ff0000".into()),
            ("brass".into(), "#00ff00".into()),
        ]),
        finish: Some(Finish {
            metallic: 0.5,
            roughness: 0.3,
            glow: 0.0,
        }),
    }
}

#[test]
fn a_skin_is_shared_and_taken_off_again() {
    let mut app = app();
    let (a, b) = (
        robot(&mut app, Who::leader(), 0.0),
        robot(&mut app, Who::leader(), 2.0),
    );
    app.update();
    let (torso, eyes) =
        (drawn(&app, a, Part::Torso), drawn(&app, a, Part::EyeL));
    for robot in [a, b] {
        app.world_mut().entity_mut(robot).insert(Wears {
            skin: Some(skin()),
            ..default()
        });
    }
    app.update();

    let worn = drawn(&app, a, Part::Torso);
    assert_eq!(worn, drawn(&app, b, Part::Torso));
    let materials =
        app.world().resource::<Assets<StandardMaterial>>();
    let paint = materials.get(&worn[0].1).unwrap();
    assert_eq!(paint.base_color, Color::linear_rgb(1.0, 0.0, 0.0));
    assert_eq!(paint.metallic, 0.5);
    let brass = materials.get(&worn[1].1).unwrap().base_color;
    let brass = brass.to_linear();
    assert!(
        brass.green > 1.0 && brass.red == 0.0 && brass.blue == 0.0
    );
    // The skin sets neither the steel of the limbs nor the eyes.
    assert_eq!(drawn(&app, a, Part::EyeL), eyes);
    let kit = app.world().resource::<Kit>();
    assert_eq!(drawn(&app, a, Part::ArmL)[0].1, kit.matte);

    app.world_mut().entity_mut(a).remove::<Wears>();
    app.update();
    assert_eq!(drawn(&app, a, Part::Torso), torso);
    assert_eq!(drawn(&app, b, Part::Torso), worn);
}

#[test]
fn a_worn_mesh_replaces_the_parts_it_names() {
    let mut mb = Mb::new();
    mb.rbox(
        Vec3::Y * 0.17,
        Vec3::splat(0.12),
        0.02,
        Quat::IDENTITY,
        Vec3::new(0.8, 0.2, 0.1),
        0.0,
    );
    let head = mb.build();
    let defaults = default_parts(&Palette::default(), Who::leader());
    let named = |part: &str| {
        &defaults.iter().find(|(name, _)| *name == part).unwrap().1
    };
    let (arm, leg) = (named("arm"), named("leg"));
    let bytes = Glb::new()
        .mesh("head", &head, Transform::IDENTITY)
        .mesh("arm_l", arm, Transform::IDENTITY)
        .mesh("leg", leg, Transform::IDENTITY)
        .write();

    let mut app = App::new();
    let dir = Dir::default();
    let root = dir.clone();
    app.register_asset_source(
        AssetSourceId::from("looks"),
        AssetSourceBuilder::new(move || {
            Box::new(MemoryAssetReader { root: root.clone() })
        }),
    );
    app.add_plugins((
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
        plugin,
    ))
    .init_resource::<Clock>();
    // As `App::run` does: the glTF loader is registered here.
    app.finish();
    app.cleanup();
    crate::stock(app.world_mut());
    dir.insert_asset(Path::new("robot.glb"), bytes);
    let gltf = app
        .world()
        .resource::<AssetServer>()
        .load::<Gltf>("looks://robot.glb");
    let id = robot(&mut app, Who::leader(), 0.0);
    app.update();
    let (torso, arm_r) =
        (drawn(&app, id, Part::Torso), drawn(&app, id, Part::ArmR));
    app.world_mut().entity_mut(id).insert(Wears {
        mesh: Some(gltf),
        ..default()
    });
    for _ in 0..500 {
        app.update();
        if !app.world().get::<Fitted>(id).unwrap().waiting {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }

    let world = app.world();
    let meshes = world.resource::<Assets<Mesh>>();
    let matte = world.resource::<Kit>().matte.clone();
    for (part, made) in [
        (Part::Head, &head),
        (Part::ArmL, arm),
        (Part::LegL, leg),
        (Part::LegR, leg),
    ] {
        let shapes = drawn(&app, id, part);
        let (mesh, material, shown) = &shapes[0];
        assert_eq!(
            meshes.get(mesh).map(Mesh::count_vertices),
            Some(made.count_vertices()),
            "{part:?}"
        );
        assert_eq!(*material, matte);
        assert!(shown);
        // The rest of a replaced part is hidden.
        assert!(shapes[1..].iter().all(|(.., shown)| !shown));
    }
    assert_eq!(drawn(&app, id, Part::ArmR), arm_r);
    assert_eq!(drawn(&app, id, Part::Torso), torso);
}

fn clip() -> Clip {
    let turn = Move {
        turn: [0.0, 0.0, 90.0],
        shift: [0.0; 3],
    };
    let lift = Move {
        turn: [0.0; 3],
        shift: [0.0, 0.1, 0.0],
    };
    let still = || Key {
        at: 0.0,
        parts: BTreeMap::new(),
    };
    Clip {
        seconds: 2.0,
        every: 10.0,
        keys: vec![
            still(),
            Key {
                at: 1.0,
                parts: BTreeMap::from([
                    ("arm_r".into(), turn),
                    ("head".into(), lift),
                ]),
            },
            Key { at: 2.0, ..still() },
        ],
    }
}

#[test]
fn a_clip_moves_only_the_parts_it_names() {
    let mut app = app();
    let (plain, worn) = (
        robot(&mut app, Who::leader(), 0.0),
        robot(&mut app, Who::leader(), 0.0),
    );
    app.world_mut().entity_mut(worn).insert(Wears {
        clip: Some(clip()),
        ..default()
    });
    app.update();
    app.world_mut().get_mut::<Gesture>(worn).unwrap().play(0.0);
    app.world_mut().resource_mut::<Clock>().t = 1.0;
    app.update();

    let world = app.world();
    let parts = |robot: Entity| {
        let ids = &world.get::<Keeper>(robot).unwrap().parts;
        [ids.arm_r, ids.head, ids.arm_l, ids.body, ids.leg_l]
            .map(|e| *world.get::<Transform>(e).unwrap())
    };
    let (was, now) = (parts(plain), parts(worn));
    let turned =
        was[0].rotation * Quat::from_rotation_z(90f32.to_radians());
    assert!(now[0].rotation.angle_between(turned) < 1e-4);
    let lifted = now[1].translation - was[1].translation;
    assert!((lifted - Vec3::Y * 0.1).length() < 1e-5);
    assert_eq!(now[2..], was[2..]);
}

#[test]
fn play_emote_starts_the_clip_of_the_agent_named() {
    let mut app = app();
    let (ada, bo) = (
        robot(&mut app, Who::citizen(3), 0.0),
        robot(&mut app, Who::citizen(4), 2.0),
    );
    for (robot, name) in [(ada, "ada"), (bo, "bo")] {
        app.world_mut().entity_mut(robot).insert((
            Wearer(name.into()),
            Wears {
                clip: Some(clip()),
                ..default()
            },
        ));
    }
    app.update();
    app.world_mut().resource_mut::<Clock>().t = 3.0;
    app.world_mut().trigger(PlayEmote("ada".into()));
    app.update();
    let since =
        |robot| app.world().get::<Gesture>(robot).unwrap().since;
    assert_eq!(since(ada), Some(3.0));
    assert_eq!(since(bo), None);
}

#[test]
fn an_idle_robot_plays_its_clip_at_most_every_every() {
    let motion = Motion::default();
    let clip = clip();
    let starts = |seed: u32, idle: bool| {
        let mut gesture = Gesture::default();
        let mut starts = Vec::new();
        for i in 0..2000 {
            let now = i as f32 * 0.05;
            let was = gesture.since;
            gesture.moves(&clip, now, idle, seed, &motion);
            if gesture.since.is_some() && gesture.since != was {
                starts.push(now);
            }
        }
        starts
    };
    let (a, b) = (starts(5, true), starts(6, true));
    assert!(a[0] < clip.every && a[0] != b[0]);
    for w in a.windows(2) {
        let gap = w[1] - w[0];
        assert!(gap >= clip.every);
        assert!(
            gap <= clip.every * (1.0 + motion.emote_spread) + 0.05
        );
    }
    assert!(starts(5, false).is_empty());
}
