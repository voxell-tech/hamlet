//! The chief's portrait on its page: its mascot itself, live, drawn
//! from the front by a camera of its own. The camera sees only a
//! layer the mascot is put on as well as the world's, so the portrait
//! has nothing behind the mascot, and its own light, so it is lit
//! the same by day and by night. It draws only while a portrait is
//! shown.

use bevy::app::{App, PostUpdate, Startup};
use bevy::asset::{Assets, Handle};
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{
    Camera, Camera3d, ClearColorConfig, ImageRenderTarget,
    PerspectiveProjection, Projection, RenderTarget,
};
use bevy::color::Color;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::Children;
use bevy::ecs::query::{With, Without};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{
    Commands, Local, Query, Res, ResMut, Single,
};
use bevy::image::Image;
use bevy::light::{AmbientLight, DirectionalLight};
use bevy::math::Vec3;
use bevy::pbr::{MeshMaterial3d, StandardMaterial};
use bevy::render::render_resource::TextureFormat;
use bevy::transform::TransformSystems;
use bevy::transform::components::{GlobalTransform, Transform};
use hamlet_realm::chief::{ChiefSettings, Mascot};

/// The layer only the portrait's camera and light see.
const LAYER: usize = 6;

/// The side of the portrait's picture, in pixels: twice the side it
/// is shown at, so it is sharp on a dense screen and smoothed on
/// another.
const SIDE: u32 = 300;

/// How much of the mascot's height the picture spans, from its feet
/// to over the flame above its head, and where along that height the
/// camera looks.
const SPAN: f32 = 1.45;
const LOOK_AT: f32 = 0.56;

/// The camera's field of view, in degrees, and how far above the
/// mascot's middle it looks down from, in its heights.
const FOV: f32 = 26.0;
const RISE: f32 = 0.18;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(Startup, set_up).add_systems(
        PostUpdate,
        (pose, aim).chain().before(TransformSystems::Propagate),
    );
}

/// The picture the portrait is drawn into.
#[derive(Resource)]
pub(crate) struct Portrait {
    pub picture: Handle<Image>,
}

/// On the node a portrait is shown in: the chief it is of.
#[derive(Component, Clone, Copy)]
pub(crate) struct Sitter(pub Entity);

#[derive(Component)]
struct PortraitCamera;

#[derive(Component)]
struct PortraitLight;

/// The picture, and the camera and light that draw it, idle.
fn set_up(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let picture = images.add(Image::new_target_texture(
        SIDE,
        SIDE,
        TextureFormat::Rgba8UnormSrgb,
        None,
    ));
    let layer = RenderLayers::layer(LAYER);
    commands.spawn((
        PortraitCamera,
        Camera3d::default(),
        Camera {
            is_active: false,
            // Before the world's picture and the panels.
            order: -2,
            // Clear, so the frame it is shown in shows behind it.
            clear_color: ClearColorConfig::Custom(Color::NONE),
            ..Camera::default()
        },
        RenderTarget::Image(ImageRenderTarget {
            handle: picture.clone(),
            scale_factor: 1.0,
        }),
        Projection::Perspective(PerspectiveProjection {
            fov: FOV.to_radians(),
            ..PerspectiveProjection::default()
        }),
        AmbientLight {
            color: Color::WHITE,
            brightness: 900.0,
            ..AmbientLight::default()
        },
        layer.clone(),
        Transform::default(),
    ));
    commands.spawn((
        PortraitLight,
        DirectionalLight {
            illuminance: 6000.0,
            shadow_maps_enabled: false,
            ..DirectionalLight::default()
        },
        layer,
        Transform::default(),
    ));
    commands.insert_resource(Portrait { picture });
}

/// Puts the mascot of the chief shown on the portrait's layer as well
/// as the world's, and takes the one shown before off it.
fn pose(
    sitters: Query<&Sitter>,
    mascots: Query<(Entity, &Mascot)>,
    children: Query<&Children>,
    mut parts: Query<
        (Entity, Option<&mut RenderLayers>),
        (
            With<MeshMaterial3d<StandardMaterial>>,
            Without<PortraitCamera>,
        ),
    >,
    mut posed: Local<Option<Entity>>,
    mut commands: Commands,
) {
    let sitter = sitters.iter().next().map(|sitter| sitter.0);
    let mascot = sitter.and_then(|chief| {
        mascots
            .iter()
            .find(|(_, mascot)| mascot.chief == chief)
            .map(|(entity, _)| entity)
    });
    if let Some(was) = *posed
        && Some(was) != mascot
    {
        for part in children.iter_descendants(was) {
            if let Ok((_, Some(mut layers))) = parts.get_mut(part) {
                *layers = layers.clone().without(LAYER);
            }
        }
    }
    *posed = mascot;
    let Some(mascot) = mascot else {
        return;
    };
    // Each frame, for parts the mascot dresses anew.
    for part in children.iter_descendants(mascot) {
        match parts.get_mut(part) {
            Ok((_, Some(mut layers)))
                if !layers
                    .intersects(&RenderLayers::layer(LAYER)) =>
            {
                *layers = layers.clone().with(LAYER);
            }
            Ok((entity, None)) => {
                commands.entity(entity).try_insert(
                    RenderLayers::from_layers(&[0, LAYER]),
                );
            }
            _ => {}
        }
    }
}

/// Points the camera at the front of the mascot of the chief shown,
/// its light over its shoulder, and lets it draw only while one is.
fn aim(
    set: Res<ChiefSettings>,
    sitters: Query<&Sitter>,
    mascots: Query<(&Mascot, &GlobalTransform)>,
    camera: Single<
        (&mut Camera, &mut Transform),
        (With<PortraitCamera>, Without<PortraitLight>),
    >,
    lamp: Single<&mut Transform, With<PortraitLight>>,
) {
    let (mut camera, mut eye) = camera.into_inner();
    let sitter = sitters.iter().next().map(|sitter| sitter.0);
    let at = sitter.and_then(|chief| {
        mascots
            .iter()
            .find(|(mascot, _)| mascot.chief == chief)
            .map(|(_, at)| at)
    });
    let Some(at) = at else {
        if camera.is_active {
            camera.is_active = false;
        }
        return;
    };
    if !camera.is_active {
        camera.is_active = true;
    }
    let (scale, turn, foot) = at.to_scale_rotation_translation();
    let tall = set.mascot_box.y * scale.y;
    // The way its face looks, level.
    let front = (turn * Vec3::Z).with_y(0.0).normalize_or(Vec3::Z);
    let middle = foot + Vec3::Y * tall * LOOK_AT;
    let away = tall * SPAN * 0.5 / (FOV.to_radians() * 0.5).tan();
    let from = middle + front * away + Vec3::Y * tall * RISE;
    *eye =
        Transform::from_translation(from).looking_at(middle, Vec3::Y);
    // From over the left shoulder, as the camera sees it.
    let side = eye.right() * -0.8;
    *lamp.into_inner() =
        Transform::from_translation(middle + front + side + Vec3::Y)
            .looking_at(middle, Vec3::Y);
}
