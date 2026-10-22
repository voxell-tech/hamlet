//! The pictures the costumier's Asset stall shows of each asset: the
//! asset worn by the realm's own robot or house on a plinth, built of
//! the realm plugin's meshes and looks, drawn offscreen from evenly
//! spaced angles, or for an emote, at moments of it as the view turns
//! a little. Nothing of the asset but these pictures leaves the
//! server for the store.

mod house;
mod robot;

use std::f32::consts::TAU;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Result, bail};
use bevy::app::ScheduleRunnerPlugin;
use bevy::asset::RenderAssetUsages;
use bevy::camera::{
    Exposure, Hdr, ImageRenderTarget, PerspectiveProjection,
    RenderTarget,
};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::{CascadeShadowConfigBuilder, NotShadowCaster};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::post_process::bloom::{Bloom, BloomPrefilter};
use bevy::prelude::*;
use bevy::render::RenderPlugin;
use bevy::render::render_resource::{Face, TextureFormat};
use bevy::render::view::screenshot::{
    Screenshot, ScreenshotCaptured,
};
use bevy::window::{ExitCondition, WindowResolution};
use bevy::winit::WinitPlugin;
use hamlet_engine_sdk::scene::{
    AlphaMode as Blend, MeshData, Pbr, Rgba,
};
use image::ImageEncoder;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use looks::wardrobe::{Category, Emote, Skin, Target};
use realm::bake::{Frame, Glb, Land, Look, WorldTuning};

use crate::catalog::Entry;
use crate::previews::house::House;
use crate::previews::robot::Robot;

/// The size of each picture.
pub const SIZE: UVec2 = UVec2::new(512, 512);

/// The most bytes and pixels a side the server takes of a picture.
pub const MOST_BYTES: usize = 400 * 1024;
pub const MOST_SIDE: u32 = 2048;

/// The backdrop: the warm dark of the store's panels.
const BACKDROP: Color = Color::srgb(0.11, 0.09, 0.07);

/// The colour of the plinth a subject stands on.
const PLINTH: Color = Color::srgb(0.42, 0.36, 0.29);

/// Updates between placing a frame and its picture, and those the
/// picture has to be written in.
const SETTLE: u32 = 3;
const PATIENCE: u32 = 600;

/// The seconds between pictures of an emote of a hamlet, and how far
/// into it the first is.
const EMOTE_STEP: f32 = 8.0 / 60.0;
const EMOTE_FROM: f32 = 4.0;

/// The land every house is built on.
const SEED: u64 = 7;

/// One asset to draw: its entry, its file, and where its pictures go.
pub struct Job {
    pub entry: Entry,
    pub file: Vec<u8>,
    pub out: PathBuf,
}

/// A node of what a frame draws: the node it hangs from, its place
/// in it, what it draws and whether that casts shadows.
#[derive(Clone)]
pub struct Node {
    pub parent: Option<usize>,
    pub at: Frame,
    pub drawn: Option<Drawn>,
    pub shadows: bool,
}

/// A mesh drawn in a material of a look.
#[derive(Clone, Copy)]
pub struct Drawn {
    pub form: Form,
    pub look: Pbr,
}

/// A mesh of a subject, by its place in its meshes, or a ball of
/// radius 1.
#[derive(Clone, Copy, PartialEq)]
pub enum Form {
    Built(usize),
    Ball,
}

/// Draws `frames` pictures of each job as PNGs named by their number,
/// then returns. Each picture not drawn, or too large for the server,
/// is named on stderr, and is an error.
pub fn render(jobs: Vec<Job>, frames: u32) -> Result<()> {
    let (tuning, pal) =
        WorldTuning::read().map_err(anyhow::Error::msg)?;
    let land = Land::new(SEED, tuning.generator.clone(), pal);
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            // A window entity with no winit behind it.
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
    );
    let image = Image::new_target_texture(
        SIZE.x,
        SIZE.y,
        TextureFormat::Rgba8UnormSrgb,
        None,
    );
    let image =
        app.world_mut().resource_mut::<Assets<Image>>().add(image);
    app.insert_resource(Shoot {
        jobs,
        frames,
        tuning,
        land,
        target: RenderTarget::Image(ImageRenderTarget {
            handle: image,
            scale_factor: 1.0,
        }),
        at: 0,
        step: Step::Start,
        wait: 0,
        subject: None,
        meshes: Vec::new(),
        saved: 0,
        failed: Vec::new(),
    })
    .add_systems(Startup, stage)
    .add_systems(Update, direct);
    if app.run().is_error() {
        bail!("some pictures were not drawn");
    }
    Ok(())
}

/// What is being drawn, and how far along.
#[derive(Resource)]
struct Shoot {
    jobs: Vec<Job>,
    frames: u32,
    tuning: WorldTuning,
    land: Land,
    target: RenderTarget,
    /// The job drawn.
    at: usize,
    step: Step,
    /// Updates left before the next step.
    wait: u32,
    subject: Option<Subject>,
    meshes: Vec<Handle<Mesh>>,
    /// Pictures written of the job drawn.
    saved: u32,
    /// Why each asset that failed did.
    failed: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Step {
    Start,
    /// The picture of this number is placed next.
    Frame(u32),
    /// The picture of this number is taken.
    Shoot(u32),
    /// The picture of this number is being written, for so many
    /// updates.
    Saving(u32, u32),
}

/// A robot or a house wearing the asset drawn.
enum Subject {
    Robot(Robot),
    House(House),
}

/// What every part of the subject carries, so it is taken away for
/// the next.
#[derive(Component)]
struct Staged;

/// The camera of the stage.
#[derive(Component)]
struct Eye;

/// The key light, and the fill opposite it.
#[derive(Component)]
struct Sun;

#[derive(Component)]
struct Fill;

/// The stage's camera and lights.
fn stage(mut commands: Commands, shoot: Res<Shoot>) {
    commands.spawn((
        Sun,
        DirectionalLight {
            shadow_maps_enabled: true,
            ..default()
        },
        CascadeShadowConfigBuilder {
            num_cascades: 1,
            minimum_distance: 0.5,
            maximum_distance: 60.0,
            first_cascade_far_bound: 60.0,
            ..default()
        }
        .build(),
    ));
    commands.spawn((Fill, DirectionalLight::default()));
    commands.spawn((
        Eye,
        Camera3d::default(),
        Camera {
            order: -3,
            clear_color: ClearColorConfig::Custom(BACKDROP),
            ..default()
        },
        shoot.target.clone(),
        Hdr,
        Tonemapping::TonyMcMapface,
    ));
}

impl Subject {
    /// The subject of `job`, its look read as the realm reads it.
    fn of(job: &Job, shoot: &Shoot) -> Result<Self> {
        let (entry, file) = (&job.entry, &job.file);
        let skin = match entry.category {
            Category::Skin => Some(Skin::read(file, entry.target)?),
            _ => None,
        };
        let emote = match entry.category {
            Category::Emote => Some(Emote::read(file, entry.target)?),
            _ => None,
        };
        let glb = match entry.category {
            Category::Mesh => {
                Some(Glb::read(file).map_err(anyhow::Error::msg)?)
            }
            _ => None,
        };
        Ok(match entry.target {
            Target::Agent => {
                let clip = match emote {
                    Some(Emote::Clip(clip)) => Some(clip),
                    _ => None,
                };
                Subject::Robot(Robot::new(
                    &shoot.land.pal,
                    shoot.tuning.wear,
                    (skin.as_ref(), glb.as_ref(), clip),
                ))
            }
            Target::Hamlet => {
                let candle = match glows(emote.as_ref()) {
                    true => shoot.tuning.looks.night.candle,
                    false => shoot.tuning.looks.day.candle,
                };
                Subject::House(
                    House::new(
                        &shoot.tuning,
                        &shoot.land,
                        (skin, glb.as_ref(), emote),
                        candle,
                    )
                    .map_err(anyhow::Error::msg)?,
                )
            }
        })
    }

    fn meshes(&self) -> &[MeshData] {
        match self {
            Subject::Robot(robot) => &robot.meshes,
            Subject::House(house) => &house.meshes,
        }
    }

    fn nodes(&self, t: f32) -> Vec<Node> {
        match self {
            Subject::Robot(robot) => robot.nodes(t),
            Subject::House(house) => house.nodes(t),
        }
    }
}

/// True for an emote seen at its best after dark: what glows.
fn glows(emote: Option<&Emote>) -> bool {
    matches!(emote, Some(Emote::Lanterns(_) | Emote::Fireflies(_)))
}

/// Where the eye looks at a subject of `target`: its middle, how far
/// away and how high above, and the radius of its plinth.
fn framing(target: Target) -> (Vec3, f32, f32, f32) {
    match target {
        Target::Agent => (Vec3::Y * 0.62, 4.6, 1.6, 1.1),
        Target::Hamlet => (Vec3::Y * 1.8, 18.5, 7.0, 4.6),
    }
}

/// The eye at `turn` about a subject of `target`.
fn eye(target: Target, turn: f32) -> Transform {
    let (middle, far, high, _) = framing(target);
    let from = Vec3::new(turn.sin() * far, high, turn.cos() * far);
    Transform::from_translation(middle + from)
        .looking_at(middle, Vec3::Y)
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

impl Job {
    /// The view's turn about its subject and the clock time drawn in
    /// picture `n` of `frames`.
    fn moment(&self, n: u32, frames: u32) -> (f32, f32) {
        let share = n as f32 / frames as f32;
        let (target, category) =
            (self.entry.target, self.entry.category);
        if category != Category::Emote {
            return (TAU * share, 0.0);
        }
        let turn = emote_turn(target) + 0.9 * share;
        match Emote::read(&self.file, target) {
            Ok(Emote::Clip(clip)) => (turn, clip.seconds * share),
            _ => (turn, EMOTE_FROM + EMOTE_STEP * n as f32),
        }
    }

    /// True when its subject is seen after dark: a hamlet's emote
    /// that glows.
    fn night(&self) -> bool {
        self.entry.target == Target::Hamlet
            && glows(
                Emote::read(&self.file, self.entry.target)
                    .ok()
                    .as_ref(),
            )
    }
}

/// Raises each job's subject and takes its pictures one after
/// another; then leaves.
fn direct(world: &mut World) {
    let mut shoot =
        world.remove_resource::<Shoot>().expect("a shoot");
    if shoot.wait > 0 {
        shoot.wait -= 1;
        world.insert_resource(shoot);
        return;
    }
    let Some(job) = shoot.jobs.get(shoot.at) else {
        for why in &shoot.failed {
            eprintln!("{why}");
        }
        world.write_message(match shoot.failed.is_empty() {
            true => AppExit::Success,
            false => AppExit::error(),
        });
        world.insert_resource(shoot);
        return;
    };
    let id = job.entry.id;
    match shoot.step {
        Step::Start => {
            std::fs::create_dir_all(&job.out).ok();
            match Subject::of(job, &shoot) {
                Ok(subject) => {
                    let mut meshes =
                        world.resource_mut::<Assets<Mesh>>();
                    shoot.meshes = subject
                        .meshes()
                        .iter()
                        .map(|data| meshes.add(mesh(data)))
                        .collect();
                    shoot.light(world, job.night(), job.entry.target);
                    shoot.subject = Some(subject);
                    shoot.saved = 0;
                    shoot.step = Step::Frame(0);
                }
                Err(why) => {
                    shoot.failed.push(format!("{id}: {why}"));
                    shoot.at += 1;
                }
            }
        }
        Step::Frame(n) if n < shoot.frames => {
            let subject = shoot.subject.as_ref().expect("a subject");
            let (turn, t) = job.moment(n, shoot.frames);
            let nodes = subject.nodes(t);
            Shoot::unstage(world);
            shoot.raise(world, &nodes, job.entry.target);
            let at = eye(job.entry.target, turn);
            let mut eyes =
                world.query_filtered::<&mut Transform, With<Eye>>();
            for mut eye in eyes.iter_mut(world) {
                *eye = at;
            }
            shoot.step = Step::Shoot(n);
            shoot.wait = SETTLE;
        }
        Step::Shoot(n) => {
            let path = job.out.join(format!("{n}.png"));
            let label = format!("{id} {n}");
            world.spawn(Screenshot(shoot.target.clone())).observe(
                move |captured: On<ScreenshotCaptured>,
                      mut shoot: ResMut<Shoot>| {
                    let saved =
                        png(&captured.image).and_then(|png| {
                            std::fs::write(&path, png)?;
                            Ok(())
                        });
                    match saved {
                        Ok(()) => shoot.saved += 1,
                        Err(why) => shoot
                            .failed
                            .push(format!("{label}: {why}")),
                    }
                },
            );
            shoot.step = Step::Saving(n, 0);
        }
        Step::Saving(n, waited) => {
            if shoot.saved > n {
                shoot.step = Step::Frame(n + 1);
            } else if waited > PATIENCE {
                shoot.failed.push(format!("{id} {n}: never drawn"));
                shoot.step = Step::Frame(shoot.frames);
            } else {
                shoot.step = Step::Saving(n, waited + 1);
            }
        }
        Step::Frame(_) => {
            Shoot::unstage(world);
            eprintln!("drew {id}");
            shoot.subject = None;
            shoot.meshes.clear();
            shoot.at += 1;
            shoot.step = Step::Start;
        }
    }
    world.insert_resource(shoot);
}

/// `image` as a PNG, its alpha dropped, refusing one larger than the
/// server takes.
fn png(image: &Image) -> Result<Vec<u8>> {
    let rgb = image.clone().try_into_dynamic()?.to_rgb8();
    if rgb.width().max(rgb.height()) > MOST_SIDE {
        bail!("{} pixels a side is too many", rgb.width());
    }
    let mut png = Vec::new();
    PngEncoder::new_with_quality(
        &mut png,
        CompressionType::Best,
        FilterType::Adaptive,
    )
    .write_image(
        rgb.as_raw(),
        rgb.width(),
        rgb.height(),
        image::ExtendedColorType::Rgb8,
    )?;
    if png.len() > MOST_BYTES {
        bail!("{} bytes is too large", png.len());
    }
    Ok(png)
}

impl Shoot {
    /// Lights the stage as the realm's sky looks by day, or after
    /// dark when `night`, for a subject of `target`.
    fn light(&self, world: &mut World, night: bool, target: Target) {
        let looks = &self.tuning.looks;
        let look: &Look =
            if night { &looks.night } else { &looks.day };
        let (middle, far, _, _) = framing(target);
        let lens = self.tuning.camera.lens(look, far);
        let colour = |c: Vec3| {
            Color::LinearRgba(LinearRgba::rgb(c.x, c.y, c.z))
        };
        let rgba = |(r, g, b, a): Rgba| LinearRgba::new(r, g, b, a);
        let mut suns = world
        .query_filtered::<(&mut DirectionalLight, &mut Transform), With<Sun>>();
        for (mut light, mut at) in suns.iter_mut(world) {
            light.color = colour(look.sun);
            light.illuminance = look.sun_lux;
            *at = Transform::from_translation(middle)
                .looking_to(look.sun_dir, Vec3::Y);
        }
        let fill = Vec3::new(
            -look.sun_dir.x,
            -0.5,
            -look.sun_dir.z * 0.4 - 0.5,
        );
        let mut fills = world
        .query_filtered::<(&mut DirectionalLight, &mut Transform), With<Fill>>();
        for (mut light, mut at) in fills.iter_mut(world) {
            light.color = colour(look.fill);
            light.illuminance = look.fill_lux;
            *at = Transform::default().looking_to(fill, Vec3::Y);
        }
        let fov = match lens.projection {
            hamlet_engine_sdk::scene::Projection::Perspective((
                fov,
                _,
                _,
            )) => fov,
            hamlet_engine_sdk::scene::Projection::Orthographic(_) => {
                0.44
            }
        };
        let (ambient, nits) = lens.ambient;
        let eye = world
            .query_filtered::<Entity, With<Eye>>()
            .single(world)
            .expect("an eye");
        let mut eye = world.entity_mut(eye);
        eye.insert((
            Projection::Perspective(PerspectiveProjection {
                fov,
                near: 0.1,
                far: 200.0,
                ..default()
            }),
            Exposure {
                ev100: lens.exposure,
            },
            AmbientLight {
                color: Color::LinearRgba(rgba(ambient)),
                brightness: nits,
                affects_lightmapped_meshes: true,
            },
            Msaa::Sample4,
        ));
        match lens.bloom {
            Some(bloom) => {
                eye.insert(Bloom {
                    intensity: bloom.intensity,
                    low_frequency_boost: bloom.low_frequency_boost,
                    prefilter: BloomPrefilter {
                        threshold: bloom.threshold,
                        threshold_softness: 0.0,
                    },
                    ..Bloom::NATURAL
                });
            }
            None => {
                eye.remove::<Bloom>();
            }
        }
    }

    /// Takes the subject and its plinth away.
    fn unstage(world: &mut World) {
        let staged = world
        .query_filtered::<Entity, (With<Staged>, Without<ChildOf>)>()
        .iter(world)
        .collect::<Vec<_>>();
        for entity in staged {
            world.entity_mut(entity).despawn();
        }
    }

    /// Raises `nodes` of the subject drawn on the plinth of a subject
    /// of `target`.
    fn raise(
        &self,
        world: &mut World,
        nodes: &[Node],
        target: Target,
    ) {
        let (_, _, _, radius) = framing(target);
        let plinth = (
            Mesh3d(
                world
                    .resource_mut::<Assets<Mesh>>()
                    .add(Cylinder::new(radius, 0.2)),
            ),
            MeshMaterial3d(
                world.resource_mut::<Assets<StandardMaterial>>().add(
                    StandardMaterial {
                        base_color: PLINTH,
                        perceptual_roughness: 0.95,
                        ..default()
                    },
                ),
            ),
        );
        world.spawn((
            Staged,
            plinth,
            Transform::from_xyz(0.0, -0.1, 0.0),
        ));
        let ball = world
            .resource_mut::<Assets<Mesh>>()
            .add(Mesh::from(Sphere::new(1.0)));
        let mut raised = Vec::<Entity>::with_capacity(nodes.len());
        for node in nodes {
            let at = Transform {
                translation: node.at.at,
                rotation: node.at.turn,
                scale: node.at.size,
            };
            let mut entity =
                world.spawn((Staged, at, Visibility::default()));
            if let Some(parent) = node.parent {
                entity.insert(ChildOf(raised[parent]));
            }
            if let Some(drawn) = node.drawn {
                let shape = match drawn.form {
                    Form::Built(at) => self.meshes[at].clone(),
                    Form::Ball => ball.clone(),
                };
                entity.insert(Mesh3d(shape));
                if !node.shadows {
                    entity.insert(NotShadowCaster);
                }
            }
            let entity = entity.id();
            if let Some(drawn) = node.drawn {
                let material = world
                    .resource_mut::<Assets<StandardMaterial>>()
                    .add(material(drawn.look));
                world
                    .entity_mut(entity)
                    .insert(MeshMaterial3d(material));
            }
            raised.push(entity);
        }
    }
}

/// `data` as a Bevy mesh, as the engine makes one.
fn mesh(data: &MeshData) -> Mesh {
    let threes = |list: &[f32]| list.as_chunks::<3>().0.to_vec();
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_POSITION,
        threes(&data.positions),
    )
    .with_inserted_indices(Indices::U32(data.indices.clone()));
    if data.normals.is_empty() {
        mesh.compute_normals();
    } else {
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_NORMAL,
            threes(&data.normals),
        );
    }
    if !data.uvs.is_empty() {
        let pairs = data.uvs.as_chunks::<2>().0.to_vec();
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, pairs);
    }
    if !data.colors.is_empty() {
        let fours = data.colors.as_chunks::<4>().0.to_vec();
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, fours);
    }
    mesh
}

/// `look` as a Bevy material, as the engine makes one.
fn material(look: Pbr) -> StandardMaterial {
    let rgba = |(r, g, b, a): Rgba| LinearRgba::new(r, g, b, a);
    StandardMaterial {
        base_color: Color::LinearRgba(rgba(look.base_color)),
        emissive: rgba(look.emissive),
        perceptual_roughness: look.roughness.clamp(0.0, 1.0),
        metallic: look.metallic.clamp(0.0, 1.0),
        alpha_mode: match look.alpha_mode {
            Blend::Opaque => AlphaMode::Opaque,
            Blend::Mask => AlphaMode::Mask(look.alpha_cutoff),
            Blend::Blend => AlphaMode::Blend,
            Blend::Premultiplied => AlphaMode::Premultiplied,
            Blend::Add => AlphaMode::Add,
            Blend::Multiply => AlphaMode::Multiply,
        },
        unlit: look.unlit,
        double_sided: look.double_sided,
        cull_mode: (!look.double_sided).then_some(Face::Back),
        fog_enabled: look.fog,
        ..default()
    }
}
