//! The material's preview, floating over the canvas in a card of the
//! editor's: a mesh with the material, shaded by the hour's
//! directional lights through the realm's camera and post processing,
//! as on a house in the realm. Dragging turns the shape and scrolling
//! brings it nearer. The surface colour a graph reads
//! is a palette colour picked here, as a mesh in the realm gives its
//! own.

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
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Local, Query, Res, ResMut};
use bevy::image::Image;
use bevy::light::DirectionalLight;
use bevy::math::{EulerRot, Quat, Vec3};
use bevy::mesh::{Mesh, Mesh3d, Meshable};
use bevy::pbr::{
    DistanceFog, ExtendedMaterial, MaterialExtension, MaterialPlugin,
    MeshMaterial3d, StandardMaterial,
};
use bevy::picking::events::{PointerDrag, PointerScroll};
use bevy::reflect::TypePath;
use bevy::render::render_resource::{
    AsBindGroup, TextureDimension, TextureFormat, TextureUsages,
};
use bevy::shader::{Shader, ShaderRef};
use bevy::shape::{Cuboid, Rectangle, Sphere};
use bevy::time::Time;
use bevy::transform::components::Transform;
use bevy::ui::widget::ViewportNode;
use bevy::ui::{
    Display, Node, Overflow, PositionType, UiRect, Val, percent, px,
};
use bevy::window::{PrimaryWindow, Window};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, column, label, row,
};
use bevy_fynix::{ScopedExt, ViewExt, resource};
use hamlet_graph_editor::Editor;
use hamlet_material::surface_shader;
use hamlet_realm::palette::Palette as Colours;
use hamlet_realm::sky::{self, CameraSettings, Hourly};
use hamlet_ui::styles::{self, description, note};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::{AnyView, Cx};

use super::Material;
use super::swatch::Slots;
use crate::graphs::assets::{COLOURS, palette_colour};

/// The render layer only the material preview's camera, lights and
/// shape are on: apart from the blueprint preview's 8 and the
/// realm's debug overlay's 7.
const LAYER: usize = 9;

/// The shader the preview's material runs, written again when the
/// graph's code changes.
const SHADER: Handle<Shader> =
    uuid_handle!("2f7d9a3e-55c1-4b8e-8d0a-7e4c3b9f6a21");

/// How far the camera stands from the shape before it is zoomed: the
/// sphere, a unit across, fills about two thirds of the picture.
const DISTANCE: f32 = 3.4;

/// The shapes the material can be seen on.
const SHAPES: [&str; 3] = ["Sphere", "Cube", "Quad"];

pub(super) fn plugin(app: &mut App) {
    app.add_plugins(MaterialPlugin::<SurfaceMaterial>::default())
        .init_resource::<Stand>()
        .add_systems(Startup, spawn)
        .add_systems(
            Update,
            (open, shade, feed, (spin, pose).chain()),
        );
}

/// A graph's material: the standard material, its surface worked out
/// by the graph.
pub(super) type SurfaceMaterial =
    ExtendedMaterial<StandardMaterial, SurfaceExt>;

/// The uniforms the graph's shader reads beyond the standard
/// material.
#[derive(Asset, AsBindGroup, TypePath, Debug, Clone, Default)]
pub(super) struct SurfaceExt {
    #[uniform(100)]
    slots: Slots,
}

impl MaterialExtension for SurfaceExt {
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(SHADER)
    }
}

/// Marks the frame the preview is drawn in.
#[derive(Component)]
struct Shown;

/// The camera, the shape it faces, the shape's meshes and its
/// material.
#[derive(Resource)]
struct Stage {
    camera: Entity,
    key: Entity,
    shape: Entity,
    meshes: [Handle<Mesh>; SHAPES.len()],
    material: Handle<SurfaceMaterial>,
}

/// How the shape is shown: which one, how it is turned, how near,
/// whether it turns by itself, and the palette colour it gives.
#[derive(Resource)]
struct Stand {
    shape: usize,
    yaw: f32,
    pitch: f32,
    zoom: f32,
    spin: bool,
    colour: usize,
}

impl Default for Stand {
    fn default() -> Self {
        Self {
            shape: 0,
            yaw: 0.6,
            pitch: 0.35,
            zoom: 1.0,
            spin: true,
            colour: 0,
        }
    }
}

/// The picture, square to the card it floats in, with the error
/// over its top and the switches over its foot.
pub(super) fn view() -> AnyView {
    AnyView::new(|cx: &mut Cx| {
        let theme = cx.theme();
        let picture = column((error(), switches()))
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

/// Why the graph does not compile, over the preview's corner, while
/// it shows the last shader that did.
fn error() -> AnyView {
    AnyView::new(|cx: &mut Cx| {
        let theme = cx.theme();
        cx.build(
            column((description(resource::<Editor<Material>, _>(
                |editor| {
                    editor
                        .errors
                        .first()
                        .map(|error| format!("Not compiled: {error}"))
                        .unwrap_or_default()
                },
            )),))
            .position(PositionType::Absolute)
            .inset(UiRect {
                left: px(8.0),
                right: px(8.0),
                top: px(8.0),
                bottom: Val::Auto,
            })
            .padding(UiRect::axes(px(8.0), px(4.0)))
            .fill(theme.palette.card)
            .radius(theme.space.round)
            .display(resource::<Editor<Material>, _>(|editor| {
                if editor.errors.is_empty() {
                    Display::None
                } else {
                    Display::Flex
                }
            })),
        )
    })
}

/// The shape, the palette colour and the spin, in the preview's lower
/// corner.
fn switches() -> AnyView {
    let switch = |tip: &'static str,
                  words: fn(&Stand) -> String,
                  flip: fn(&mut Stand)| {
        button(label(resource::<Stand, _>(words)))
            .padding(UiRect::axes(px(8.0), px(2.0)))
            .tooltip(move || note(tip))
            .rules(styles::button)
            .tagged(Variant::Default)
            .on_activate(move |world| {
                flip(&mut world.resource_mut::<Stand>());
            })
    };
    row((
        switch(
            "The shape the material is shown on",
            |stand| SHAPES[stand.shape].to_owned(),
            |stand| stand.shape = (stand.shape + 1) % SHAPES.len(),
        ),
        switch(
            "The palette colour the mesh gives, which surface \
             colour reads",
            |stand| COLOURS[stand.colour].replace('_', " "),
            |stand| stand.colour = (stand.colour + 1) % COLOURS.len(),
        ),
        switch(
            "Whether the shape turns by itself",
            |stand| {
                format!(
                    "Spin: {}",
                    if stand.spin { "on" } else { "off" }
                )
            },
            |stand| stand.spin = !stand.spin,
        ),
    ))
    .gap(4.0)
    .position(PositionType::Absolute)
    .inset(UiRect {
        left: px(8.0),
        bottom: px(8.0),
        right: Val::Auto,
        top: Val::Auto,
    })
    .boxed()
}

fn spawn(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<SurfaceMaterial>>,
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
        // The realm's lens, but with a near plane for a shape within
        // reach, not a hamlet from afar.
        .insert(Projection::Perspective(PerspectiveProjection {
            fov: settings.fov.to_radians(),
            near: 0.05,
            far: 100.0,
            ..PerspectiveProjection::default()
        }))
        .id();
    let key = commands
        .spawn((
            Hourly::Key,
            DirectionalLight {
                shadow_maps_enabled: true,
                ..DirectionalLight::default()
            },
            Transform::default(),
            RenderLayers::layer(LAYER),
        ))
        .id();
    commands.spawn((
        Hourly::Fill,
        DirectionalLight::default(),
        Transform::default(),
        RenderLayers::layer(LAYER),
    ));
    let shapes = [
        meshes.add(Sphere::new(0.5).mesh().uv(64, 32)),
        meshes.add(Cuboid::from_length(0.8)),
        meshes.add(Rectangle::from_length(1.1)),
    ];
    let material = materials.add(SurfaceMaterial {
        base: StandardMaterial::default(),
        extension: SurfaceExt::default(),
    });
    let shape = commands
        .spawn((
            Mesh3d(shapes[0].clone()),
            MeshMaterial3d(material.clone()),
            Transform::default(),
            RenderLayers::layer(LAYER),
        ))
        .id();
    commands.insert_resource(Stage {
        camera,
        key,
        shape,
        meshes: shapes,
        material,
    });
}

/// Draws the preview while its frame is in the panel, and lets it be
/// turned by dragging.
fn open(
    mut commands: Commands,
    stage: Option<Res<Stage>>,
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
        commands.entity(frame).observe(turn).observe(zoom);
        *watched = Some(frame);
    }
    if let Ok(mut camera) = cameras.get_mut(stage.camera)
        && camera.is_active != frame.is_some()
    {
        camera.is_active = frame.is_some();
    }
}

/// Turns the shape by any button's drag.
fn turn(
    drag: On<PointerDrag>,
    frames: Query<(), With<Shown>>,
    mut stand: ResMut<Stand>,
) {
    if frames.get(drag.event_target()).is_err() {
        return;
    }
    stand.yaw += drag.delta.x * 0.01;
    stand.pitch =
        (stand.pitch + drag.delta.y * 0.01).clamp(-1.4, 1.4);
}

fn zoom(scroll: On<PointerScroll>, mut stand: ResMut<Stand>) {
    stand.zoom =
        (stand.zoom * (1.0 - scroll.y * 0.1)).clamp(0.4, 3.0);
}

/// Turns the shape a little every frame while it spins.
fn spin(time: Res<Time>, mut stand: ResMut<Stand>) {
    if stand.spin {
        stand.yaw += time.delta_secs() * 0.4;
    }
}

/// Shows the shape, its palette colour and the camera as the stand
/// says.
fn pose(
    mut commands: Commands,
    stage: Option<Res<Stage>>,
    stand: Res<Stand>,
    colours: Option<Res<Colours>>,
    set: Res<CameraSettings>,
    mut materials: ResMut<Assets<SurfaceMaterial>>,
    mut shapes: Query<(&mut Transform, &mut Mesh3d), With<Mesh3d>>,
    mut cameras: Query<
        (&mut Transform, &mut DistanceFog),
        bevy::ecs::query::Without<Mesh3d>,
    >,
) {
    let Some(stage) = stage else {
        return;
    };
    if !stand.is_changed() {
        return;
    }
    if let Ok((mut transform, mut mesh)) = shapes.get_mut(stage.shape)
    {
        transform.rotation = Quat::from_euler(
            EulerRot::XYZ,
            stand.pitch,
            stand.yaw,
            0.0,
        );
        if mesh.0 != stage.meshes[stand.shape] {
            mesh.0 = stage.meshes[stand.shape].clone();
        }
    }
    let distance = DISTANCE * stand.zoom;
    if let Ok((mut transform, mut fog)) =
        cameras.get_mut(stage.camera)
    {
        *transform = Transform::from_xyz(0.0, 0.0, distance)
            .looking_at(Vec3::ZERO, Vec3::Y);
        fog.falloff = sky::fog_for(&set, distance + 20.0);
    }
    commands
        .entity(stage.key)
        .try_insert(sky::shadows_for(&set, distance));
    let colour = colours
        .as_deref()
        .and_then(|colours| {
            palette_colour(colours, COLOURS[stand.colour])
        })
        .unwrap_or(Vec3::splat(0.8));
    if let Some(mut material) = materials.get_mut(&stage.material) {
        material.base.base_color =
            Color::linear_rgb(colour.x, colour.y, colour.z);
    }
}

/// Writes the preview's shader again when the graph's code changes.
fn shade(
    editor: Res<Editor<Material>>,
    mut shaders: ResMut<Assets<Shader>>,
    mut shaded: Local<Option<u64>>,
) {
    if *shaded == Some(editor.got.version) {
        return;
    }
    let Some(compiled) = editor.got.compiled() else {
        return;
    };
    *shaded = Some(editor.got.version);
    let _ = shaders.insert(
        SHADER.id(),
        Shader::from_wesl(
            surface_shader(compiled),
            "hamlet_material/surface.wesl",
        ),
    );
}

/// Fills the material's slots with the graph's values, again each
/// frame while the graph reads the time.
fn feed(
    editor: Res<Editor<Material>>,
    stage: Option<Res<Stage>>,
    time: Res<Time>,
    mut materials: ResMut<Assets<SurfaceMaterial>>,
) {
    let (Some(stage), Some(compiled)) =
        (stage, editor.got.compiled())
    else {
        return;
    };
    if !compiled.reads_time() && !editor.is_changed() {
        return;
    }
    if let Some(mut material) = materials.get_mut(&stage.material) {
        material.extension.slots = Slots::of(
            &compiled
                .values(&editor.sheet.graph, time.elapsed_secs()),
        );
    }
}
