//! A camera's image shown in a UI node, sized by the layout, and the
//! pointer over a view, in a viewport or behind the dock, told to the
//! scene's owner.

use std::collections::HashSet;

use bevy::app::{App, Update};
use bevy::asset::Assets;
use bevy::camera::Camera;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::message::{MessageReader, MessageWriter};
use bevy::ecs::query::{Added, Changed, With};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::schedule::common_conditions::resource_exists;
use bevy::ecs::system::{Commands, Query, Res, ResMut};
use bevy::image::Image;
use bevy::input::ButtonInput;
use bevy::input::mouse::{MouseButton, MouseScrollUnit, MouseWheel};
use bevy::math::Vec2;
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::PointerId;
use bevy::render::render_resource::Extent3d;
use bevy::ui::widget::ImageNode;
use bevy::ui::{
    ComputedNode, Node, RelativeCursorPosition, percent, px,
};
use bevy::window::{PrimaryWindow, Window};
use hamlet_engine_core::TuningPlugin;
use hamlet_engine_core::tuning::{ENGINE, Tuning, TuningFile};
use serde::Deserialize;

use crate::pick::{PointerButton, SceneEvent, SceneInput, pick};
use crate::scene::SceneCamera;

pub(crate) fn plugin(app: &mut App) {
    app.add_plugins(TuningPlugin::<RenderTuning>::default())
        .add_systems(
            Update,
            (
                (show, size, size_behind, activate).chain(),
                (track, pick).chain().run_if(
                    resource_exists::<ButtonInput<MouseButton>>,
                ),
            )
                .chain(),
        );
}

/// How scenes are shown.
#[derive(Resource, Clone, PartialEq, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderTuning {
    /// The least height of a viewport, in the interface's pixels.
    pub viewport_least: f32,
    /// The widest and tallest image a camera draws into, in pixels.
    pub viewport_most: u32,
    /// Pixels the pointer may move between a press and its release
    /// for them to be a click.
    pub click_slop: f32,
    /// Pixels of a wheel turned by the pixel that make a line.
    pub wheel_line: f32,
    /// Levels above a viewport made to grow with it, to the first
    /// that grows already.
    pub grow_levels: usize,
    /// Computes and passes of all plugins timed on the GPU at once,
    /// read as the engine starts.
    pub timed_slots: u32,
}

impl Tuning for RenderTuning {
    const FILE: TuningFile = ENGINE;
    const SECTION: Option<&'static str> = Some("render");
}

/// On a UI node: shows the image `camera`, a scene's camera, draws
/// into, filling the node.
#[derive(Component, Clone, Copy, Debug)]
#[require(RelativeCursorPosition)]
pub struct Viewport {
    pub camera: Entity,
}

/// On a scene's camera: the pointer over its view.
#[derive(Component, Default, Debug)]
pub(crate) struct ViewPointer {
    /// Where, in the logical pixels of what the camera draws into.
    pub(crate) at: Option<Vec2>,
    /// The interface's pixels for one of [`ViewPointer::at`].
    scale: f32,
    pub(crate) hovered: Option<Entity>,
    /// A click to pick at [`ViewPointer::at`].
    pub(crate) click: bool,
    down: [bool; 3],
    down_at: Vec2,
    size: Vec2,
}

const BUTTONS: [(MouseButton, PointerButton); 3] = [
    (MouseButton::Left, PointerButton::Primary),
    (MouseButton::Right, PointerButton::Secondary),
    (MouseButton::Middle, PointerButton::Middle),
];

/// Shows each new viewport's image, filling it, and has what holds
/// it grow so it has room.
fn show(
    viewports: Query<(Entity, &Viewport), Added<Viewport>>,
    cameras: Query<&SceneCamera>,
    mut nodes: Query<&mut Node>,
    parents: Query<&ChildOf>,
    tuning: Res<RenderTuning>,
    mut commands: Commands,
) {
    for (shown, viewport) in &viewports {
        let Some(image) = cameras
            .get(viewport.camera)
            .ok()
            .and_then(|camera| camera.image.clone())
        else {
            continue;
        };
        commands.entity(shown).insert(ImageNode::new(image));
        if let Ok(mut node) = nodes.get_mut(shown) {
            node.flex_grow = 1.0;
            node.width = percent(100.0);
            node.min_height = px(tuning.viewport_least);
        }
        let mut at = shown;
        for _ in 0..tuning.grow_levels {
            let Ok(parent) = parents.get(at) else {
                break;
            };
            at = parent.parent();
            let Ok(mut node) = nodes.get_mut(at) else {
                break;
            };
            if node.flex_grow > 0.0 {
                break;
            }
            node.flex_grow = 1.0;
        }
    }
}

/// Gives each viewport's camera an image of the viewport's size.
fn size(
    viewports: Query<
        (&Viewport, &ComputedNode),
        Changed<ComputedNode>,
    >,
    mut cameras: Query<(&SceneCamera, &mut ViewPointer)>,
    mut images: ResMut<Assets<Image>>,
    tuning: Res<RenderTuning>,
    mut inputs: MessageWriter<SceneInput>,
) {
    for (viewport, computed) in &viewports {
        let Ok((camera, mut pointer)) =
            cameras.get_mut(viewport.camera)
        else {
            continue;
        };
        let most = tuning.viewport_most;
        let pixels = computed.size().round().as_uvec2();
        let (width, height) =
            (pixels.x.clamp(1, most), pixels.y.clamp(1, most));
        if let Some(mut image) = camera
            .image
            .as_ref()
            .and_then(|image| images.get_mut(image))
            && image.size() != bevy::math::UVec2::new(width, height)
        {
            image.resize(Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            });
        }
        let size = computed.size() * computed.inverse_scale_factor();
        if size != pointer.size {
            pointer.size = size;
            inputs.write(SceneInput {
                owner: camera.owner,
                event: SceneEvent::Resized {
                    view: camera.key,
                    size,
                },
            });
        }
    }
}

/// Tells the owner of a camera behind the dock the window's size, as
/// a viewport's is told.
fn size_behind(
    windows: Query<&Window, With<PrimaryWindow>>,
    mut cameras: Query<(&Camera, &SceneCamera, &mut ViewPointer)>,
    mut inputs: MessageWriter<SceneInput>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let size = Vec2::new(window.width(), window.height());
    for (camera, scene_camera, mut pointer) in &mut cameras {
        if scene_camera.image.is_some()
            || !camera.is_active
            || pointer.size == size
        {
            continue;
        }
        pointer.size = size;
        inputs.write(SceneInput {
            owner: scene_camera.owner,
            event: SceneEvent::Resized {
                view: scene_camera.key,
                size,
            },
        });
    }
}

/// Draws a camera into its image only while a viewport shows it.
fn activate(
    viewports: Query<&Viewport>,
    mut cameras: Query<(Entity, &SceneCamera, &mut Camera)>,
) {
    let shown = viewports
        .iter()
        .map(|viewport| viewport.camera)
        .collect::<HashSet<_>>();
    for (entity, scene_camera, mut camera) in &mut cameras {
        if scene_camera.image.is_none() {
            continue;
        }
        let active = shown.contains(&entity);
        if camera.is_active != active {
            camera.is_active = active;
        }
    }
}

/// Tells each scene's owner of the pointer over its views: where it
/// moved, its buttons and the wheel. A view behind the dock has the
/// pointer where no UI node is under it.
fn track(
    viewports: Query<(
        &Viewport,
        &RelativeCursorPosition,
        &ComputedNode,
    )>,
    windows: Query<&Window, With<PrimaryWindow>>,
    hovers: Option<Res<HoverMap>>,
    mut cameras: Query<(
        Entity,
        &Camera,
        &SceneCamera,
        &mut ViewPointer,
    )>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut wheels: MessageReader<MouseWheel>,
    tuning: Res<RenderTuning>,
    mut inputs: MessageWriter<SceneInput>,
) {
    let turned = wheels.read().fold(Vec2::ZERO, |sum, wheel| {
        let by = Vec2::new(wheel.x, wheel.y);
        sum + match wheel.unit {
            MouseScrollUnit::Line => by,
            MouseScrollUnit::Pixel => by / tuning.wheel_line,
        }
    });
    let window_free = hovers.is_none_or(|hovers| {
        hovers
            .get(&PointerId::Mouse)
            .is_none_or(|over| over.is_empty())
    });
    let cursor = windows
        .single()
        .ok()
        .and_then(Window::cursor_position)
        .filter(|_| window_free);
    for (entity, camera, scene_camera, mut pointer) in &mut cameras {
        let over = if scene_camera.image.is_some() {
            viewports
                .iter()
                .find(|(viewport, ..)| viewport.camera == entity)
                .and_then(|(_, relative, computed)| {
                    let at = relative.normalized?;
                    relative.cursor_over.then(|| {
                        (
                            (at + 0.5) * computed.size(),
                            computed.inverse_scale_factor(),
                        )
                    })
                })
        } else if camera.is_active {
            cursor.map(|at| (at, 1.0))
        } else {
            None
        };
        let owner = scene_camera.owner;
        let view = scene_camera.key;
        let mut tell = |event| {
            inputs.write(SceneInput { owner, event });
        };
        let Some((at, scale)) = over else {
            pointer.at = None;
            // A press begun over the view is let go anywhere.
            for (index, (button, which)) in BUTTONS.iter().enumerate()
            {
                if pointer.down[index]
                    && buttons.just_released(*button)
                {
                    pointer.down[index] = false;
                    let at = pointer.down_at * pointer.scale;
                    tell(SceneEvent::Pressed {
                        view,
                        button: *which,
                        down: false,
                        at,
                    });
                }
            }
            continue;
        };
        let shown = at * scale;
        if pointer.at != Some(at) {
            let by = pointer
                .at
                .map_or(Vec2::ZERO, |was| (at - was) * scale);
            tell(SceneEvent::Moved {
                view,
                at: shown,
                by,
            });
        }
        pointer.at = Some(at);
        pointer.scale = scale;
        for (index, (button, which)) in BUTTONS.iter().enumerate() {
            if buttons.just_pressed(*button) {
                pointer.down[index] = true;
                if index == 0 {
                    pointer.down_at = at;
                }
                tell(SceneEvent::Pressed {
                    view,
                    button: *which,
                    down: true,
                    at: shown,
                });
            } else if pointer.down[index]
                && buttons.just_released(*button)
            {
                pointer.down[index] = false;
                let still = (at - pointer.down_at).length() * scale
                    <= tuning.click_slop;
                pointer.click |= index == 0 && still;
                tell(SceneEvent::Pressed {
                    view,
                    button: *which,
                    down: false,
                    at: shown,
                });
            }
        }
        if turned != Vec2::ZERO {
            tell(SceneEvent::Turned { view, by: turned });
        }
    }
}
