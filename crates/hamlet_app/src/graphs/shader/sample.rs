//! A material's preview in the ASSETS panel's details: a sphere
//! shaded by the material's shader with the material's values, under
//! the hour's lights through the realm's camera, as the shader's own
//! preview is. It turns by itself, and dragging turns it.

use bevy::app::{App, Startup, Update};
use bevy::asset::{
    Asset, Assets, Handle, RenderAssetUsages, uuid_handle,
};
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{
    Camera, ImageRenderTarget, PerspectiveProjection, Projection,
    RenderTarget,
};
use bevy::color::Color;
use bevy::ecs::change_detection::DetectChanges;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::event::EntityEvent;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Local, Query, Res, ResMut};
use bevy::image::Image;
use bevy::light::DirectionalLight;
use bevy::math::{EulerRot, Quat, Vec3};
use bevy::mesh::{Mesh, Mesh3d, Meshable};
use bevy::pbr::{
    ExtendedMaterial, MaterialExtension, MaterialPlugin,
    MeshMaterial3d, StandardMaterial,
};
use bevy::picking::events::PointerDrag;
use bevy::reflect::TypePath;
use bevy::render::render_resource::{
    AsBindGroup, TextureDimension, TextureFormat, TextureUsages,
};
use bevy::shader::{Shader, ShaderRef};
use bevy::shape::Sphere;
use bevy::time::Time;
use bevy::transform::components::Transform;
use bevy::ui::widget::ViewportNode;
use bevy::ui::{Node, Overflow, percent};
use bevy::window::{PrimaryWindow, Window};
use bevy_fynix::views::{BehaviorExt, FrameProps, column};
use hamlet_graph::Graph;
use hamlet_realm::palette::Palette as Colours;
use hamlet_realm::sky::{self, CameraSettings, Hourly};
use hamlet_shader::{Compiled, compile, surface_shader};
use hamlet_ui::{AnyView, Cx};

use super::swatch::Slots;
use crate::graphs::assets::{COLOURS, palette_colour};

/// The render layer only the material preview's camera, lights and
/// sphere are on: apart from the shader preview's 9 and the
/// thumbnails' 10.
const LAYER: usize = 11;

/// The shader the preview's material runs, written again when the
/// material's shader's code changes.
const SHADER: Handle<Shader> =
    uuid_handle!("a4c1e7d2-0b9f-4e63-9a58-3f2d6c8e1b74");

/// How far the camera stands from the sphere.
const DISTANCE: f32 = 2.6;

pub(super) fn plugin(app: &mut App) {
    app.add_plugins(MaterialPlugin::<SampleMaterial>::default())
        .init_resource::<Sample>()
        .init_resource::<Turn>()
        .add_systems(Startup, spawn)
        .add_systems(Update, (open, shade, pose));
}

/// The shader the details show, its inputs' defaults what the
/// material makes them, or none while no material with a shader is
/// shown.
#[derive(Resource, Default)]
pub(crate) struct Sample {
    pub graph: Option<Graph>,
}

type SampleMaterial = ExtendedMaterial<StandardMaterial, SampleExt>;

#[derive(Asset, AsBindGroup, TypePath, Debug, Clone, Default)]
struct SampleExt {
    #[uniform(100)]
    slots: Slots,
}

impl MaterialExtension for SampleExt {
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(SHADER)
    }
}

/// Marks the frame the preview is drawn in.
#[derive(Component)]
struct Shown;

/// The camera, the sphere and its material.
#[derive(Resource)]
struct Stage {
    camera: Entity,
    sphere: Entity,
    material: Handle<SampleMaterial>,
}

/// How the sphere is turned.
#[derive(Resource)]
struct Turn {
    yaw: f32,
    pitch: f32,
}

impl Default for Turn {
    fn default() -> Self {
        Self {
            yaw: 0.6,
            pitch: 0.35,
        }
    }
}

/// The picture, as wide as the details and square.
pub(crate) fn view() -> AnyView {
    AnyView::new(|cx: &mut Cx| {
        let theme = cx.theme();
        let picture = column(())
            .width(percent(100.0))
            .overflow(Overflow::clip())
            .fill(theme.palette.input)
            .radius(theme.space.round)
            .tagged(Shown);
        let picture = match cx
            .world
            .get_resource::<Stage>()
            .map(|stage| stage.camera)
        {
            Some(camera) => {
                cx.build(picture.seeded(ViewportNode::new(camera)))
            }
            None => cx.build(picture),
        };
        if let Some(mut node) = cx.world.get_mut::<Node>(picture) {
            node.aspect_ratio = Some(1.0);
        }
        picture
    })
}

fn spawn(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<SampleMaterial>>,
    window: Query<&Window, With<PrimaryWindow>>,
    settings: Option<Res<CameraSettings>>,
) {
    let mut image = Image::new_uninit(
        Default::default(),
        TextureDimension::D2,
        TextureFormat::Bgra8UnormSrgb,
        RenderAssetUsages::all(),
    );
    image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING
        | TextureUsages::COPY_DST
        | TextureUsages::RENDER_ATTACHMENT;
    let handle = images.add(image);
    let scale =
        window.iter().next().map_or(1.0, Window::scale_factor);
    let settings = settings.as_deref().cloned().unwrap_or_default();
    let camera = commands
        .spawn((
            sky::view(&settings),
            Camera {
                // Before the world's camera and the window's.
                order: -3,
                is_active: false,
                ..Camera::default()
            },
            RenderTarget::Image(ImageRenderTarget {
                handle,
                scale_factor: scale,
            }),
            RenderLayers::layer(LAYER),
            Transform::from_xyz(0.0, 0.0, DISTANCE)
                .looking_at(Vec3::ZERO, Vec3::Y),
        ))
        .insert(Projection::Perspective(PerspectiveProjection {
            fov: settings.fov.to_radians(),
            near: 0.05,
            far: 100.0,
            ..PerspectiveProjection::default()
        }))
        .id();
    commands.spawn((
        Hourly::Key,
        DirectionalLight::default(),
        Transform::default(),
        RenderLayers::layer(LAYER),
    ));
    commands.spawn((
        Hourly::Fill,
        DirectionalLight::default(),
        Transform::default(),
        RenderLayers::layer(LAYER),
    ));
    let material = materials.add(SampleMaterial {
        base: StandardMaterial::default(),
        extension: SampleExt::default(),
    });
    let sphere = commands
        .spawn((
            Mesh3d(meshes.add(Sphere::new(0.5).mesh().uv(64, 32))),
            MeshMaterial3d(material.clone()),
            Transform::default(),
            RenderLayers::layer(LAYER),
        ))
        .id();
    commands.insert_resource(Stage {
        camera,
        sphere,
        material,
    });
}

/// Draws the preview while its frame is in the details and a shader
/// is given, and lets it be turned by dragging.
fn open(
    mut commands: Commands,
    stage: Option<Res<Stage>>,
    sample: Res<Sample>,
    shown: Query<Entity, With<Shown>>,
    mut cameras: Query<&mut Camera>,
    mut watched: Local<Option<Entity>>,
) {
    let Some(stage) = stage else {
        return;
    };
    let frame = shown.iter().next();
    if let Some(frame) = frame
        && *watched != Some(frame)
    {
        commands.entity(frame).observe(turn);
        *watched = Some(frame);
    }
    let active = frame.is_some() && sample.graph.is_some();
    if let Ok(mut camera) = cameras.get_mut(stage.camera)
        && camera.is_active != active
    {
        camera.is_active = active;
    }
}

fn turn(
    drag: On<PointerDrag>,
    frames: Query<(), With<Shown>>,
    mut turn: ResMut<Turn>,
) {
    if frames.get(drag.event_target()).is_err() {
        return;
    }
    turn.yaw += drag.delta.x * 0.01;
    turn.pitch = (turn.pitch + drag.delta.y * 0.01).clamp(-1.4, 1.4);
}

/// Compiles the sample when it changes, writes the shader again when
/// its code changes, and fills the slots with its values, again each
/// frame while it reads the time.
fn shade(
    sample: Res<Sample>,
    stage: Option<Res<Stage>>,
    time: Res<Time>,
    colours: Option<Res<Colours>>,
    mut shaders: ResMut<Assets<Shader>>,
    mut materials: ResMut<Assets<SampleMaterial>>,
    mut got: Local<Option<Compiled>>,
) {
    let Some(stage) = stage else {
        return;
    };
    let Some(graph) = &sample.graph else {
        return;
    };
    if sample.is_changed() {
        let Ok(compiled) = compile(graph) else {
            return;
        };
        if got.as_ref().map(|old| &old.code) != Some(&compiled.code) {
            let _ = shaders.insert(
                SHADER.id(),
                Shader::from_wesl(
                    surface_shader(&compiled),
                    "hamlet_shader/surface.wesl",
                ),
            );
        }
        *got = Some(compiled);
    } else if !got.as_ref().is_some_and(Compiled::reads_time) {
        return;
    }
    let Some(compiled) = got.as_ref() else {
        return;
    };
    let Some(mut material) = materials.get_mut(&stage.material)
    else {
        return;
    };
    material.extension.slots =
        Slots::of(&compiled.values(graph, time.elapsed_secs()));
    let colour = colours
        .as_deref()
        .and_then(|colours| palette_colour(colours, COLOURS[0]))
        .unwrap_or(Vec3::splat(0.8));
    material.base.base_color =
        Color::linear_rgb(colour.x, colour.y, colour.z);
}

/// Turns the sphere a little every frame, and as dragged.
fn pose(
    stage: Option<Res<Stage>>,
    time: Res<Time>,
    mut turn: ResMut<Turn>,
    mut spheres: Query<&mut Transform, With<Mesh3d>>,
) {
    let Some(stage) = stage else {
        return;
    };
    turn.yaw += time.delta_secs() * 0.4;
    if let Ok(mut transform) = spheres.get_mut(stage.sphere) {
        transform.rotation = Quat::from_euler(
            EulerRot::XYZ,
            turn.pitch,
            turn.yaw,
            0.0,
        );
    }
}
