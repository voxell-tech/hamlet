//! The preview: what the graph puts down, as boxes in the world's
//! colours, drawn by a camera of its own into the panel and turned by
//! dragging.

use bevy::app::{App, Startup, Update};
use bevy::asset::{Assets, Handle, RenderAssetUsages};
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{
    Camera, Camera3d, ClearColorConfig, ImageRenderTarget,
    RenderTarget,
};
use bevy::color::Color;
use bevy::ecs::change_detection::DetectChanges;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Local, Query, Res, ResMut};
use bevy::image::Image;
use bevy::light::DirectionalLight;
use bevy::math::{Quat, Vec3};
use bevy::mesh::{Mesh, Mesh3d};
use bevy::pbr::{MeshMaterial3d, StandardMaterial};
use bevy::picking::events::PointerDrag;
use bevy::render::render_resource::{
    TextureDimension, TextureFormat, TextureUsages,
};
use bevy::transform::components::Transform;
use bevy::ui::widget::ViewportNode;
use bevy::ui::{Overflow, percent, px};
use bevy::window::{PrimaryWindow, Window};
use bevy_fynix::views::{BehaviorExt, frame};
use hamlet_graph::Instance;
use hamlet_realm::meshes::Meshing;
use hamlet_realm::palette::Palette as Colours;

use super::Editor;
use crate::{AnyView, Cx, View};

/// The asset name a placement is shown as when nothing is put there.
pub(super) const MARKER: &str = "marker";

/// The render layer only the preview's camera and pieces are on.
const LAYER: usize = 7;

/// The height of the preview in the panel.
const HEIGHT: f32 = 260.0;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Turn>()
        .init_resource::<Framing>()
        .add_systems(Startup, spawn)
        .add_systems(Update, (open, rebuild, aim).chain());
}

/// Marks the frame the preview is drawn in.
#[derive(Component)]
struct Shown;

/// What the graph put down, drawn.
#[derive(Component)]
struct Piece;

/// The camera that draws the preview, and the material of its pieces.
#[derive(Resource)]
struct Preview {
    camera: Entity,
    material: Handle<StandardMaterial>,
}

/// Which way the camera looks from, in radians.
#[derive(Resource)]
struct Turn {
    yaw: f32,
    pitch: f32,
}

impl Default for Turn {
    fn default() -> Self {
        Self {
            yaw: 0.8,
            pitch: 0.55,
        }
    }
}

/// The middle and the reach of what is drawn.
#[derive(Resource, Default)]
struct Framing {
    middle: Vec3,
    reach: f32,
}

pub(super) fn view() -> impl View {
    AnyView::new(|cx: &mut Cx| {
        let theme = cx.theme();
        let panel = frame()
            .width(percent(100.0))
            .height(px(HEIGHT))
            .shrink(0.0)
            .overflow(Overflow::clip())
            .radius(theme.space.round)
            .fill(theme.palette.well)
            .tagged(Shown);
        match cx
            .world
            .get_resource::<Preview>()
            .map(|preview| preview.camera)
        {
            Some(camera) => {
                cx.build(panel.seeded(ViewportNode::new(camera)))
            }
            None => cx.build(panel),
        }
    })
}

fn spawn(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    window: Query<&Window, With<PrimaryWindow>>,
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
    let camera = commands
        .spawn((
            Camera3d::default(),
            Camera {
                // Before the world's camera and the window's.
                order: -2,
                is_active: false,
                clear_color: ClearColorConfig::Custom(Color::srgb(
                    0.07, 0.06, 0.05,
                )),
                ..Camera::default()
            },
            RenderTarget::Image(ImageRenderTarget {
                handle,
                scale_factor: scale,
            }),
            RenderLayers::layer(LAYER),
            Transform::from_xyz(8.0, 6.0, 8.0)
                .looking_at(Vec3::ZERO, Vec3::Y),
        ))
        .id();
    commands.spawn((
        DirectionalLight {
            illuminance: 9_000.0,
            ..DirectionalLight::default()
        },
        Transform::from_xyz(3.0, 8.0, 5.0)
            .looking_at(Vec3::ZERO, Vec3::Y),
        RenderLayers::layer(LAYER),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 2_500.0,
            ..DirectionalLight::default()
        },
        Transform::from_xyz(-4.0, 3.0, -6.0)
            .looking_at(Vec3::ZERO, Vec3::Y),
        RenderLayers::layer(LAYER),
    ));
    let material = materials.add(StandardMaterial {
        perceptual_roughness: 0.9,
        reflectance: 0.2,
        ..StandardMaterial::default()
    });
    commands.insert_resource(Preview { camera, material });
}

/// Draws the preview while its frame is in the panel, and lets it be
/// turned by dragging.
fn open(
    mut commands: Commands,
    preview: Option<Res<Preview>>,
    shown: Query<Entity, With<Shown>>,
    mut cameras: Query<&mut Camera>,
    mut watched: Local<Option<Entity>>,
) {
    let Some(preview) = preview else {
        return;
    };
    let frame = shown.iter().next();
    if let Some(frame) = frame
        && *watched != Some(frame)
    {
        commands.entity(frame).observe(turn);
        *watched = Some(frame);
    }
    if let Ok(mut camera) = cameras.get_mut(preview.camera)
        && camera.is_active != frame.is_some()
    {
        camera.is_active = frame.is_some();
    }
}

fn turn(drag: On<PointerDrag>, mut turn: ResMut<Turn>) {
    turn.yaw -= drag.delta.x * 0.01;
    turn.pitch = (turn.pitch + drag.delta.y * 0.01).clamp(0.1, 1.45);
}

/// Draws what the graph put down again when it changes.
fn rebuild(
    mut commands: Commands,
    editor: Res<Editor>,
    preview: Option<Res<Preview>>,
    colours: Option<Res<Colours>>,
    pieces: Query<Entity, With<Piece>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut framing: ResMut<Framing>,
    mut drawn: Local<bool>,
) {
    let Some(preview) = preview else {
        return;
    };
    if !editor.is_changed() && *drawn {
        return;
    }
    *drawn = true;
    for piece in &pieces {
        commands.entity(piece).despawn();
    }
    let Ok(instances) = &editor.outcome else {
        return;
    };
    let mut meshing = Meshing::new();
    let (mut low, mut high) =
        (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for instance in instances {
        let size = instance.size * instance.frame.scale;
        let marker = instance.asset == MARKER;
        // An asset stands on its placement; a marker sits on it.
        let lift = if marker { 0.0 } else { size.y * 0.5 };
        let middle = instance.frame.translation
            + instance.frame.rotation * Vec3::new(0.0, lift, 0.0);
        meshing.rbox(
            middle,
            size * 0.5,
            0.04,
            instance.frame.rotation,
            colour(colours.as_deref(), instance),
            if marker { 0.0 } else { 0.25 },
        );
        low = low.min(middle - size);
        high = high.max(middle + size);
    }
    if instances.is_empty() {
        (low, high) = (Vec3::splat(-2.0), Vec3::splat(2.0));
    }
    let ground =
        colours.as_deref().map_or(Vec3::splat(0.3), |c| c.grass);
    let span = (high - low).max(Vec3::splat(1.0));
    meshing.rbox(
        Vec3::new(
            (low.x + high.x) * 0.5,
            -0.05,
            (low.z + high.z) * 0.5,
        ),
        Vec3::new(span.x * 0.5 + 0.5, 0.05, span.z * 0.5 + 0.5),
        0.02,
        Quat::IDENTITY,
        ground,
        0.0,
    );
    framing.middle = (low + high) * 0.5;
    framing.reach = (high - low).length() * 0.5;
    commands.spawn((
        Piece,
        Mesh3d(meshes.add(meshing.build())),
        MeshMaterial3d(preview.material.clone()),
        RenderLayers::layer(LAYER),
    ));
}

/// The colour of an asset: the world's colour its name suggests, or
/// one picked from its name.
fn colour(colours: Option<&Colours>, instance: &Instance) -> Vec3 {
    let Some(c) = colours else {
        return Vec3::splat(0.7);
    };
    let name = instance.asset.as_str();
    let named = [
        ("marker", c.ochre),
        ("pillar", c.limestone),
        ("stone", c.limestone),
        ("wall", c.plaster),
        ("roof", c.terracotta),
        ("tile", c.terracotta),
        ("post", c.timber),
        ("rail", c.timber),
        ("beam", c.timber_dark),
        ("door", c.timber_dark),
        ("moss", c.moss),
        ("slate", c.slate_roof),
    ];
    if let Some((_, colour)) =
        named.iter().find(|(word, _)| name.contains(word))
    {
        return *colour;
    }
    let others = [
        c.plaster,
        c.ochre,
        c.whitewash,
        c.limestone,
        c.terracotta,
        c.moss,
    ];
    let hash = name.bytes().fold(0usize, |hash, byte| {
        hash.wrapping_mul(31).wrapping_add(usize::from(byte))
    });
    others[hash % others.len()]
}

/// Points the camera at what is drawn, from the way it is turned.
fn aim(
    preview: Option<Res<Preview>>,
    turn: Res<Turn>,
    framing: Res<Framing>,
    mut cameras: Query<&mut Transform, With<Camera3d>>,
) {
    let Some(preview) = preview else {
        return;
    };
    if !turn.is_changed() && !framing.is_changed() {
        return;
    }
    let Ok(mut transform) = cameras.get_mut(preview.camera) else {
        return;
    };
    let toward = Vec3::new(
        turn.yaw.cos() * turn.pitch.cos(),
        turn.pitch.sin(),
        turn.yaw.sin() * turn.pitch.cos(),
    );
    let distance = framing.reach * 1.5 + 2.0;
    *transform = Transform::from_translation(
        framing.middle + toward * distance,
    )
    .looking_at(framing.middle, Vec3::Y);
}
