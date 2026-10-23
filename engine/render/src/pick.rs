//! What the person does in a scene's views, told to its owner, and
//! the watched entity under the pointer, found by casting a ray
//! through the camera at the meshes.

use std::path::PathBuf;

use bevy::camera::Camera;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::message::{Message, MessageWriter};
use bevy::ecs::system::Query;
use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
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
    /// A press let go over a watched entity within the click's slop.
    Click {
        hit: Hit,
        button: PointerButton,
        mods: Modifiers,
    },
    Moved {
        view: u32,
        at: Vec2,
        by: Vec2,
        /// The buttons held, primary, secondary and middle.
        held: [bool; 3],
        mods: Modifiers,
    },
    Pressed {
        view: u32,
        button: PointerButton,
        down: bool,
        at: Vec2,
        mods: Modifiers,
    },
    /// A key pressed or let go over the view, or while it was the
    /// last pressed.
    Key {
        view: u32,
        key: KeyCode,
        down: bool,
        mods: Modifiers,
    },
    /// A file dropped on the view.
    Dropped {
        view: u32,
        at: Vec2,
        path: PathBuf,
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

/// The keys held with what the person did. `meta` is Command on
/// macOS and the system's key elsewhere.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct Modifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub meta: bool,
}

impl Modifiers {
    /// Those `keys` holds.
    pub(crate) fn held(keys: Option<&ButtonInput<KeyCode>>) -> Self {
        let Some(keys) = keys else {
            return Self::default();
        };
        let any = |pair: [KeyCode; 2]| keys.any_pressed(pair);
        Self {
            shift: any([KeyCode::ShiftLeft, KeyCode::ShiftRight]),
            control: any([
                KeyCode::ControlLeft,
                KeyCode::ControlRight,
            ]),
            alt: any([KeyCode::AltLeft, KeyCode::AltRight]),
            meta: any([KeyCode::SuperLeft, KeyCode::SuperRight]),
        }
    }
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
        if let Some((button, mods)) = pointer.click.take()
            && let Some((entity, hit)) = found
            && watched.get(entity).is_ok_and(|(seen, _)| seen.click)
        {
            inputs.write(SceneInput {
                owner,
                event: SceneEvent::Click { hit, button, mods },
            });
        }
    }
}
