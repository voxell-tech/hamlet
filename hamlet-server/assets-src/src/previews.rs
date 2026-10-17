//! The pictures the Asset stall shows of each asset: the asset worn
//! by a robot or a house on a plinth, drawn offscreen from evenly
//! spaced angles, or for an emote, at moments of it as the view turns
//! a little. Nothing of the asset but these pictures leaves the
//! server for the store.

use std::f32::consts::TAU;
use std::path::{Path, PathBuf};
use std::time::Duration;

use bevy::app::ScheduleRunnerPlugin;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{ImageRenderTarget, RenderTarget};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::gltf::{Gltf, GltfMesh, GltfNode};
use bevy::light::CascadeShadowConfigBuilder;
use bevy::prelude::*;
use bevy::render::RenderPlugin;
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::ColorGrading;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::time::TimeUpdateStrategy;
use bevy::window::{ExitCondition, WindowResolution};
use bevy::winit::WinitPlugin;
use hamlet_model::wardrobe::{Category, Emote, Skin, Target};
use hamlet_world::dress::{self, Closet};
use hamlet_world::generator::attire::{Attire, Shell, raise};
use hamlet_world::hamlet::HamletPlugin;
use hamlet_world::keeper::{Gesture, Kit, Wears, Who, spawn_dressed};
use hamlet_world::sky::Looks;
use hamlet_world::{Clock, WorldCamera};

use crate::catalog::Entry;

/// The size of each picture.
pub const SIZE: UVec2 = UVec2::new(400, 300);

/// The layer the stage is drawn on, which no other camera sees.
const LAYER: usize = 11;

/// Updates given a subject to stand and its meshes to load.
const SETTLE: u32 = 90;

/// Updates a subject's mesh is waited for before it is given up.
const PATIENCE: u32 = 600;

/// Updates between one picture and the next, at the least, so that
/// the last has been saved and the view moved.
const BETWEEN: u32 = 4;

/// The backdrop: the warm dark of the store's panels.
const BACKDROP: Color = Color::srgb(0.11, 0.09, 0.07);

/// One asset to draw: its entry, its file, and where its pictures go.
pub struct Job {
    pub entry: Entry,
    pub file: Vec<u8>,
    pub out: PathBuf,
}

/// Draws `frames` pictures of each job, then returns.
pub fn render(jobs: Vec<Job>, frames: u32) {
    let mut app = App::new();
    // Before the asset plugin: meshes are read from memory.
    dress::register_source(&mut app);
    app.add_plugins((
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    resolution: WindowResolution::new(SIZE.x, SIZE.y)
                        .with_scale_factor_override(1.0),
                    ..default()
                }),
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
            .set(RenderPlugin {
                synchronous_pipeline_compilation: true,
                ..default()
            })
            .disable::<WinitPlugin>()
            .add(ScheduleRunnerPlugin::run_loop(Duration::ZERO)),
        HamletPlugin,
    ))
    .insert_resource(TimeUpdateStrategy::ManualDuration(
        Duration::from_secs_f64(1.0 / 60.0),
    ));
    let image = Image::new_target_texture(
        SIZE.x,
        SIZE.y,
        TextureFormat::Rgba8UnormSrgb,
        None,
    );
    let image =
        app.world_mut().resource_mut::<Assets<Image>>().add(image);
    app.insert_resource(Shoot {
        image,
        jobs,
        frames,
        at: 0,
        step: Step::Start,
        wait: 0,
        tries: 0,
        mesh: None,
        subject: None,
    })
    .add_systems(PostStartup, stage)
    .add_systems(Update, (direct, lay).chain());
    app.run();
}

/// What is being drawn, and how far along.
#[derive(Resource)]
struct Shoot {
    image: Handle<Image>,
    jobs: Vec<Job>,
    frames: u32,
    /// The job drawn.
    at: usize,
    step: Step,
    /// Updates left before the next step.
    wait: u32,
    /// Updates spent waiting for the subject's mesh.
    tries: u32,
    /// The subject's mesh while it loads: held, or the asset server
    /// drops it and starts again.
    mesh: Option<Handle<Gltf>>,
    subject: Option<Entity>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Step {
    Start,
    /// Its mesh is loading.
    Loading,
    Settling,
    /// The picture of this number is next.
    Frame(u32),
    Done,
}

/// What every stage part carries: the stage's layer.
#[derive(Component)]
struct Staged;

/// The camera of the stage.
#[derive(Component)]
struct Eye;

/// A light of the stage, and how bright it is by day.
#[derive(Component)]
struct Lamp(f32);

/// How much of the day's light is left at dusk, when what glows after
/// dark is drawn.
const DUSK: f32 = 0.08;

/// The stage's camera, lights and plinth, and the world's camera put
/// out.
fn stage(
    mut commands: Commands,
    shoot: Res<Shoot>,
    looks: Res<Looks>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut world_cameras: Query<&mut Camera, With<WorldCamera>>,
) {
    for mut camera in &mut world_cameras {
        camera.is_active = false;
    }
    let day = &looks.day;
    let layer = RenderLayers::layer(LAYER);
    commands.spawn((
        Lamp(day.sun_lux),
        DirectionalLight {
            color: day.sun,
            illuminance: day.sun_lux,
            shadow_maps_enabled: true,
            ..default()
        },
        CascadeShadowConfigBuilder {
            num_cascades: 1,
            minimum_distance: 0.5,
            maximum_distance: 40.0,
            first_cascade_far_bound: 40.0,
            ..default()
        }
        .build(),
        Transform::default()
            .looking_to(Vec3::new(-0.5, -1.0, -0.7), Vec3::Y),
        layer.clone(),
    ));
    commands.spawn((
        Lamp(day.fill_lux),
        DirectionalLight {
            color: day.fill,
            illuminance: day.fill_lux,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::default()
            .looking_to(Vec3::new(0.6, -0.4, 0.5), Vec3::Y),
        layer.clone(),
    ));
    commands.spawn((
        Eye,
        Camera3d::default(),
        Camera {
            order: -3,
            clear_color: ClearColorConfig::Custom(BACKDROP),
            ..default()
        },
        RenderTarget::Image(ImageRenderTarget {
            handle: shoot.image.clone(),
            scale_factor: 1.0,
        }),
        Msaa::Sample4,
        Tonemapping::TonyMcMapface,
        AmbientLight {
            color: day.ambient,
            brightness: day.ambient_nits,
            affects_lightmapped_meshes: true,
        },
        ColorGrading::default(),
        Transform::from_xyz(0.0, 1.0, 3.0)
            .looking_at(Vec3::Y * 0.5, Vec3::Y),
        layer.clone(),
    ));
    // A round plinth of warm stone, under whatever stands on it.
    commands.spawn((
        Staged,
        Mesh3d(meshes.add(Cylinder::new(4.0, 0.2))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.42, 0.36, 0.29),
            perceptual_roughness: 0.95,
            ..default()
        })),
        Transform::from_xyz(0.0, -0.1, 0.0),
        layer,
    ));
}

/// Puts every part of the stage on its layer, the parts a subject
/// grows as it loads and moves among them.
fn lay(
    mut commands: Commands,
    staged: Query<Entity, With<Staged>>,
    children: Query<&Children>,
    laid: Query<(), With<RenderLayers>>,
) {
    let layer = RenderLayers::layer(LAYER);
    for root in &staged {
        for part in std::iter::once(root)
            .chain(children.iter_descendants(root))
        {
            if !laid.contains(part) {
                commands.entity(part).try_insert(layer.clone());
            }
        }
    }
}

/// Where the eye looks at a subject of `target`: its middle, how far
/// away and how high above.
fn framing(target: Target) -> (Vec3, f32, f32) {
    match target {
        Target::Agent => (Vec3::Y * 0.55, 2.4, 0.9),
        Target::Hamlet => (Vec3::Y * 1.3, 7.5, 3.0),
    }
}

/// The eye at `turn` about a subject of `target`.
fn eye(target: Target, turn: f32) -> Transform {
    let (middle, far, high) = framing(target);
    let from = Vec3::new(turn.sin() * far, high, turn.cos() * far);
    Transform::from_translation(middle + from)
        .looking_at(middle, Vec3::Y)
}

/// Raises each job's subject, waits for it to stand, and takes its
/// pictures one after another; then leaves.
fn direct(world: &mut World) {
    let mut shoot =
        world.remove_resource::<Shoot>().expect("a shoot");
    if shoot.wait > 0 {
        shoot.wait -= 1;
        world.insert_resource(shoot);
        return;
    }
    let Some(job) = shoot.jobs.get(shoot.at) else {
        world.write_message(AppExit::Success);
        world.insert_resource(shoot);
        return;
    };
    let (entry, out) = (job.entry, job.out.clone());
    match shoot.step {
        Step::Start => {
            std::fs::create_dir_all(&out).ok();
            if entry.category == Category::Mesh {
                let path = format!("{}.glb", entry.id);
                world
                    .resource::<Closet>()
                    .keep(Path::new(&path), job.file.clone());
                let path = format!("{}://{path}", dress::SOURCE);
                shoot.mesh = Some(
                    world
                        .resource::<AssetServer>()
                        .load::<Gltf>(path),
                );
            }
            shoot.step = Step::Loading;
        }
        Step::Loading => {
            let subject = raise_subject(
                world,
                &entry,
                &job.file,
                shoot.mesh.as_ref(),
            );
            match subject {
                Some(subject) => {
                    world.entity_mut(subject).insert(Staged);
                    shoot.subject = Some(subject);
                    shoot.step = Step::Settling;
                    shoot.wait = SETTLE;
                }
                None if shoot.tries > PATIENCE => {
                    let server = world.resource::<AssetServer>();
                    let states = shoot
                        .mesh
                        .as_ref()
                        .map(|gltf| server.get_load_states(gltf));
                    eprintln!(
                        "{}: its mesh never loaded ({states:?}); no \
                         pictures",
                        entry.id
                    );
                    shoot.step = Step::Done;
                }
                // Its mesh is not loaded yet.
                None => {
                    shoot.tries += 1;
                    shoot.wait = 2;
                }
            }
        }
        Step::Settling => {
            if let (Some(subject), Category::Emote) =
                (shoot.subject, entry.category)
            {
                play(world, subject);
            }
            light(world, glows(&job.file, entry.target));
            shoot.step = Step::Frame(0);
        }
        Step::Frame(n) if n < shoot.frames => {
            let (turn, between) = match entry.category {
                // An emote is seen as it plays, the view turning a
                // little the while.
                Category::Emote => (
                    emote_turn(entry.target)
                        + 0.9 * n as f32 / shoot.frames as f32,
                    emote_step(&job.file, entry.target, shoot.frames),
                ),
                _ => (TAU * n as f32 / shoot.frames as f32, BETWEEN),
            };
            let at = eye(entry.target, turn);
            let mut eyes =
                world.query_filtered::<&mut Transform, With<Eye>>();
            for mut eye in eyes.iter_mut(world) {
                *eye = at;
            }
            // The view moved this update is drawn the next: the
            // picture of it is taken then.
            let image = shoot.image.clone();
            let path = out.join(format!("{n}.png"));
            world
                .commands()
                .spawn(Screenshot(RenderTarget::Image(
                    ImageRenderTarget {
                        handle: image,
                        scale_factor: 1.0,
                    },
                )))
                .observe(save_to_disk(path));
            shoot.step = Step::Frame(n + 1);
            shoot.wait = between;
        }
        Step::Frame(_) => {
            if let Some(subject) = shoot.subject.take() {
                world.entity_mut(subject).despawn();
            }
            shoot.step = Step::Done;
            shoot.wait = BETWEEN;
        }
        Step::Done => {
            eprintln!("drew {}", entry.id);
            shoot.tries = 0;
            shoot.mesh = None;
            shoot.at += 1;
            shoot.step = Step::Start;
        }
    }
    world.insert_resource(shoot);
}

/// Where the view of an emote starts turning from: an agent's from
/// beside it, a hamlet's from before its front, where its emote
/// hangs.
fn emote_turn(target: Target) -> f32 {
    match target {
        Target::Agent => 0.5,
        Target::Hamlet => -0.45,
    }
}

/// True for an emote that is seen at its best after dark.
fn glows(file: &[u8], target: Target) -> bool {
    matches!(Emote::read(file, target), Ok(Emote::Fireflies(_)))
}

/// Lights the stage as by day, or dims it to dusk.
fn light(world: &mut World, dusk: bool) {
    let share = if dusk { DUSK } else { 1.0 };
    let mut lamps = world.query::<(&Lamp, &mut DirectionalLight)>();
    for (lamp, mut light) in lamps.iter_mut(world) {
        light.illuminance = lamp.0 * share;
    }
    let day = world.resource::<Looks>().day.ambient_nits;
    let mut eyes =
        world.query_filtered::<&mut AmbientLight, With<Eye>>();
    for mut ambient in eyes.iter_mut(world) {
        ambient.brightness = day * share.max(0.25);
    }
}

/// The updates between pictures of an emote, so that they span it.
fn emote_step(file: &[u8], target: Target, frames: u32) -> u32 {
    match Emote::read(file, target) {
        Ok(Emote::Clip(clip)) => {
            ((clip.seconds * 60.0 / frames as f32).round() as u32)
                .max(BETWEEN)
        }
        _ => BETWEEN * 2,
    }
}

/// Starts the clip of the robot `subject` now.
fn play(world: &mut World, subject: Entity) {
    let now = world.resource::<Clock>().t;
    if let Some(mut gesture) = world.get_mut::<Gesture>(subject) {
        gesture.play(now);
    }
}

/// Raises what wears the asset of `entry`, its file `file` and its
/// mesh `mesh`: a robot or a house. `None` while the mesh is loading.
fn raise_subject(
    world: &mut World,
    entry: &Entry,
    file: &[u8],
    mesh: Option<&Handle<Gltf>>,
) -> Option<Entity> {
    if let Some(gltf) = mesh
        && !dress::loaded(gltf, world.resource::<AssetServer>())
    {
        return None;
    }
    let mesh = mesh.cloned();
    let skin = match entry.category {
        Category::Skin => Skin::read(file, entry.target).ok(),
        _ => None,
    };
    let emote = match entry.category {
        Category::Emote => Emote::read(file, entry.target).ok(),
        _ => None,
    };
    Some(match entry.target {
        Target::Agent => {
            let clip = match emote {
                Some(Emote::Clip(clip)) => Some(clip),
                _ => None,
            };
            let wears = Wears { skin, mesh, clip };
            let robot =
                world.run_system_cached_with(
                    |In(wears): In<Wears>,
                     mut commands: Commands,
                     kit: Res<Kit>,
                     mut materials: ResMut<
                        Assets<StandardMaterial>,
                    >| {
                        spawn_dressed(
                            &mut commands,
                            &kit,
                            &mut materials,
                            Who::citizen(3),
                            Transform::IDENTITY,
                            wears,
                        )
                    },
                    wears,
                );
            robot.ok()?
        }
        Target::Hamlet => {
            let mesh = mesh.and_then(|gltf| {
                Shell::of(
                    &gltf,
                    world.resource::<Assets<Gltf>>(),
                    world.resource::<Assets<GltfMesh>>(),
                    world.resource::<Assets<GltfNode>>(),
                    world.resource::<Assets<Mesh>>(),
                )
            });
            let attire = Attire { skin, mesh, emote };
            raise(world, &attire, false, Transform::IDENTITY)
        }
    })
}
