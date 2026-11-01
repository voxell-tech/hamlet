//! Draws the library's meshes into the pictures of [`Thumbnails`].
//! One camera, lit by lights of its own on a render layer no other
//! camera sees, shoots each mesh asked for in turn, and again when
//! its picture is out of date.

use std::collections::VecDeque;
use std::f32::consts::FRAC_PI_4;

use bevy::app::{App, Startup, Update};
use bevy::asset::{Assets, Handle};
use bevy::camera::primitives::MeshAabb as _;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{
    Camera, Camera3d, ClearColorConfig, RenderTarget,
};
use bevy::color::Color;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::message::MessageWriter;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Res, ResMut, Single};
use bevy::image::Image;
use bevy::light::{AmbientLight, DirectionalLight};
use bevy::math::Vec3;
use bevy::mesh::{Mesh, Mesh3d};
use bevy::pbr::{MeshMaterial3d, StandardMaterial};
use bevy::render::render_resource::TextureFormat;
use bevy::time::Time;
use bevy::transform::components::Transform;
use bevy::window::RequestRedraw;
use hamlet_blueprint::Frame;
use hamlet_graph::Shape;
use hamlet_graph_editor::library::Library;
use hamlet_graph_editor::library::thumbnails::{
    Job, Key, SIDE, Thumbnails,
};
use hamlet_realm::meshes::Meshing;
use hamlet_realm::palette::Palette as Colours;

use super::assets::Meshes;
use super::preview::stand;

/// The render layer only the thumbnails' camera, lights and subject
/// are on.
const LAYER: usize = 10;

/// Frames a subject is held for the camera after the one it is first
/// lit on, so its picture has been written before the next subject.
const SETTLE: u8 = 2;

/// Which way the camera looks from, relative to the subject.
const VIEW: Vec3 = Vec3::new(1.0, 0.8, 1.4);

/// How much farther than just fitting the subject the camera stands.
const MARGIN: f32 = 1.15;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Studio>()
        .add_systems(Startup, spawn)
        .add_systems(Update, (plan, develop).chain());
}

/// The pictures waiting to be drawn, and the one being drawn.
#[derive(Resource, Default)]
struct Studio {
    queue: VecDeque<Job>,
    shot: Option<Shot>,
    /// What every subject is made of.
    material: Option<Handle<StandardMaterial>>,
}

/// A picture being drawn.
struct Shot {
    name: String,
    key: Key,
    image: Handle<Image>,
    /// What the camera is pointed at, a new one for each shot.
    subject: Entity,
    /// Frames it has been held for.
    held: u8,
}

/// The thumbnails' camera.
#[derive(Component)]
struct Lens;

fn spawn(mut commands: Commands) {
    let layer = RenderLayers::layer(LAYER);
    commands.spawn((
        Lens,
        Camera3d::default(),
        Camera {
            // Before the previews' cameras and the window's.
            order: -3,
            is_active: false,
            clear_color: ClearColorConfig::Custom(Color::NONE),
            ..Camera::default()
        },
        AmbientLight {
            brightness: 160.0,
            ..AmbientLight::default()
        },
        layer.clone(),
    ));
    commands.spawn((
        DirectionalLight::default(),
        Transform::from_xyz(3.0, 5.0, 4.0)
            .looking_at(Vec3::ZERO, Vec3::Y),
        layer.clone(),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 3_500.0,
            ..DirectionalLight::default()
        },
        Transform::from_xyz(-4.0, 2.0, -3.0)
            .looking_at(Vec3::ZERO, Vec3::Y),
        layer.clone(),
    ));
}

/// Queues the pictures that are asked for or out of date.
fn plan(
    time: Res<Time>,
    library: Res<Library>,
    mut thumbnails: ResMut<Thumbnails>,
    mut studio: ResMut<Studio>,
) {
    let now = time.elapsed();
    if thumbnails.has_work(&library, now) {
        let jobs = thumbnails.due(&library, now);
        studio.queue.extend(jobs);
    }
}

/// Ends the shot that has been held long enough, and starts the
/// next.
fn develop(
    mut commands: Commands,
    mut studio: ResMut<Studio>,
    mut thumbnails: ResMut<Thumbnails>,
    library: Res<Library>,
    colours: Option<Res<Colours>>,
    mut models: ResMut<Meshes>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    lens: Single<(Entity, &mut Camera, &mut Transform), With<Lens>>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    let (lens, mut camera, mut place) = lens.into_inner();
    if let Some(shot) = &mut studio.shot {
        redraw.write(RequestRedraw);
        if shot.held < SETTLE {
            shot.held += 1;
            return;
        }
    }
    if let Some(Shot {
        name,
        key,
        image,
        subject,
        ..
    }) = studio.shot.take()
    {
        camera.is_active = false;
        commands.entity(subject).despawn();
        thumbnails.drawn(&name, key, image);
    }
    let Some(Job { name, key, image }) = studio.queue.pop_front()
    else {
        return;
    };
    redraw.write(RequestRedraw);
    let asset = key.asset();
    let model = match (&asset.shape, library.root()) {
        (Shape::Mesh(path), Some(root)) => {
            // A picture drawn before is drawn again for a changed
            // file, which is read again.
            if image.is_some() {
                models.forget_file(path);
            }
            models.get(root, path).ok()
        }
        _ => None,
    };
    // A mesh stands as long as its file is, in a box of one.
    let scale = model.map_or(Vec3::ONE, |model| {
        let extent = model.extent();
        extent / extent.max_element().max(1e-4)
    });
    let mut meshing = Meshing::new();
    stand(
        &mut meshing,
        &Frame {
            scale,
            ..Frame::IDENTITY
        },
        &name,
        Some(asset),
        model,
        colours.as_deref(),
    );
    let mesh = meshing.build();
    let Some(aabb) = mesh.get_aabb() else {
        return;
    };
    let middle = Vec3::from(aabb.center);
    let radius = Vec3::from(aabb.half_extents).length().max(0.01);
    let distance = radius / (FRAC_PI_4 / 2.0).sin() * MARGIN;
    *place = Transform::from_translation(
        middle + VIEW.normalize() * distance,
    )
    .looking_at(middle, Vec3::Y);

    let image = image.unwrap_or_else(|| {
        images.add(Image::new_target_texture(
            SIDE,
            SIDE,
            TextureFormat::Rgba8Unorm,
            Some(TextureFormat::Rgba8UnormSrgb),
        ))
    });
    let material = studio
        .material
        .get_or_insert_with(|| {
            materials.add(StandardMaterial {
                perceptual_roughness: 0.9,
                reflectance: 0.2,
                ..StandardMaterial::default()
            })
        })
        .clone();
    let subject = commands
        .spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(material),
            Transform::default(),
            RenderLayers::layer(LAYER),
        ))
        .id();
    commands
        .entity(lens)
        .insert(RenderTarget::Image(image.clone().into()));
    camera.is_active = true;
    studio.shot = Some(Shot {
        name,
        key,
        image,
        subject,
        held: 0,
    });
}
