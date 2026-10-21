//! What the person does in a scene's views, told to its owner, and
//! the watched entity under the pointer, found by casting a ray
//! through the camera at the meshes.

use bevy::camera::Camera;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::message::{Message, MessageWriter};
use bevy::ecs::system::Query;
use bevy::math::{Vec2, Vec3};
use bevy::picking::mesh_picking::ray_cast::{
    MeshRayCast, MeshRayCastSettings, RayCastVisibility,
};
use bevy::transform::components::GlobalTransform;

use crate::scene::{SceneCamera, ScenePart};
use crate::viewport::ViewPointer;

/// What the person did in a view of `owner`'s scene.
#[derive(Message, Clone, Debug)]
pub struct SceneInput {
    pub owner: Entity,
    pub event: SceneEvent,
}

/// Places and sizes are in the interface's pixels, from the view's
/// top left; a view is named by the key of its camera's entity.
#[derive(Clone, PartialEq, Debug)]
pub enum SceneEvent {
    /// The pointer came over a watched entity.
    Over(Hit),
    /// The pointer left the watched entity of this key and copy.
    Out {
        key: u32,
        index: u32,
    },
    Click(Hit),
    Moved {
        view: u32,
        at: Vec2,
        by: Vec2,
    },
    Pressed {
        view: u32,
        button: PointerButton,
        down: bool,
        at: Vec2,
    },
    /// The wheel turned, in lines.
    Turned {
        view: u32,
        by: Vec2,
    },
    Resized {
        view: u32,
        size: Vec2,
    },
}

/// Where the pointer met a watched entity, in the world.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Hit {
    pub key: u32,
    pub index: u32,
    pub point: Vec3,
    pub normal: Vec3,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PointerButton {
    Primary,
    Secondary,
    Middle,
}

/// On an entity of a scene whose owner is told when the pointer
/// comes over it or leaves, if `hover`, and of clicks, if `click`.
#[derive(Component, Clone, Copy, PartialEq, Debug)]
pub(crate) struct Watched {
    pub(crate) hover: bool,
    pub(crate) click: bool,
}

/// Finds the watched entity under the pointer in each view, and
/// tells its owner when it changes and when it is clicked.
pub(crate) fn pick(
    mut cameras: Query<(
        &Camera,
        &GlobalTransform,
        &SceneCamera,
        &mut ViewPointer,
    )>,
    watched: Query<(&Watched, &ScenePart)>,
    mut ray_cast: MeshRayCast,
    mut inputs: MessageWriter<SceneInput>,
) {
    for (camera, place, scene_camera, mut pointer) in &mut cameras {
        let owner = scene_camera.owner;
        let theirs = |entity: Entity| {
            watched
                .get(entity)
                .is_ok_and(|(_, part)| part.owner == owner)
        };
        let found = pointer.at.and_then(|at| {
            let ray = camera.viewport_to_world(place, at).ok()?;
            let settings = MeshRayCastSettings::default()
                .with_filter(&theirs)
                .with_visibility(RayCastVisibility::Visible)
                .always_early_exit();
            let (entity, hit) =
                ray_cast.cast_ray(ray, &settings).first()?;
            let (_, part) = watched.get(*entity).ok()?;
            Some((
                *entity,
                Hit {
                    key: part.key,
                    index: part.index,
                    point: hit.point,
                    normal: hit.normal,
                },
            ))
        });
        let now = found.map(|(entity, _)| entity);
        if now != pointer.hovered {
            if let Some(was) = pointer.hovered
                && let Ok((seen, part)) = watched.get(was)
                && seen.hover
            {
                inputs.write(SceneInput {
                    owner,
                    event: SceneEvent::Out {
                        key: part.key,
                        index: part.index,
                    },
                });
            }
            if let Some((entity, hit)) = found
                && watched
                    .get(entity)
                    .is_ok_and(|(seen, _)| seen.hover)
            {
                inputs.write(SceneInput {
                    owner,
                    event: SceneEvent::Over(hit),
                });
            }
            pointer.hovered = now;
        }
        if std::mem::take(&mut pointer.click)
            && let Some((entity, hit)) = found
            && watched.get(entity).is_ok_and(|(seen, _)| seen.click)
        {
            inputs.write(SceneInput {
                owner,
                event: SceneEvent::Click(hit),
            });
        }
    }
}
