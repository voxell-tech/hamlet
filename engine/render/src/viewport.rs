//! A camera's image shown in a UI node, sized by the layout, and the
//! pointer over a view, in a viewport or behind the dock, told to the
//! scene's owner.

use std::collections::HashSet;
use std::path::PathBuf;

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
use bevy::input::keyboard::{KeyCode, KeyboardInput};
use bevy::input::mouse::{MouseButton, MouseScrollUnit, MouseWheel};
use bevy::input::{ButtonInput, ButtonState};
use bevy::math::Vec2;
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::PointerId;
use bevy::render::render_resource::Extent3d;
use bevy::time::Time;
use bevy::ui::widget::ImageNode;
use bevy::ui::{
    ComputedNode, Node, RelativeCursorPosition, percent, px,
};
use bevy::window::{FileDragAndDrop, PrimaryWindow, Window};
use hamlet_engine_core::TuningPlugin;
use hamlet_engine_core::tuning::{ENGINE, Tuning, TuningFile};
use hamlet_engine_shell::ShellKeys;
use serde::Deserialize;

use crate::pick::{
    Modifiers, PointerButton, SceneEvent, SceneInput, pick,
};
use crate::scene::SceneCamera;

pub(crate) fn plugin(app: &mut App) {
    app.add_plugins(TuningPlugin::<RenderTuning>::default())
        .init_resource::<ViewFocus>()
        .init_resource::<Drops>()
        .add_systems(
            Update,
            (
                (show, size, size_behind, activate).chain(),
                (track, pick, type_keys, drop_files).chain().run_if(
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
    /// Seconds a file dropped on the window waits for the pointer to
    /// be seen over a view, which the system may not move while it
    /// drags, before it is let lie.
    pub drop_wait: f32,
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
    /// A click to pick at [`ViewPointer::at`], of a button with the
    /// keys held.
    pub(crate) click: Option<(PointerButton, Modifiers)>,
    down: [bool; 3],
    /// Where each button went down.
    down_at: [Vec2; 3],
    size: Vec2,
}

/// The camera whose view was pressed last, which is told of keys
/// while the pointer is over no view.
#[derive(Resource, Default, Debug)]
struct ViewFocus(Option<Entity>);

/// Files dropped on the window not yet told, with the seconds each
/// may still wait for the pointer to be seen over a view.
#[derive(Resource, Default, Debug)]
struct Drops(Vec<(PathBuf, f32)>);

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
        // A viewport with no room, in a hidden tab or not laid out
        // yet, keeps its image for when it has room again.
        if let Some(mut image) = camera
            .image
            .as_ref()
            .filter(|_| !computed.is_empty())
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

/// Draws a camera into its image only while a viewport shows it with
/// room to draw in.
fn activate(
    viewports: Query<(&Viewport, &ComputedNode)>,
    mut cameras: Query<(Entity, &SceneCamera, &mut Camera)>,
) {
    // A viewport in a tab behind another is kept, laid out with no
    // room.
    let shown = viewports
        .iter()
        .filter(|(_, computed)| !computed.is_empty())
        .map(|(viewport, _)| viewport.camera)
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
        Entity,
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
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mut focus: ResMut<ViewFocus>,
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
    let hovered = hovers
        .as_ref()
        .and_then(|hovers| hovers.get(&PointerId::Mouse))
        .filter(|over| !over.is_empty());
    let cursor = windows
        .single()
        .ok()
        .and_then(Window::cursor_position)
        .filter(|_| hovered.is_none());
    let mods = Modifiers::held(keys.as_deref());
    let held = BUTTONS.map(|(button, _)| buttons.pressed(button));
    let mut pressed_any = false;
    for (entity, camera, scene_camera, mut pointer) in &mut cameras {
        let over = if scene_camera.image.is_some() {
            viewports
                .iter()
                .find(|(_, viewport, ..)| viewport.camera == entity)
                .and_then(|(node, _, relative, computed)| {
                    let at = relative.normalized?;
                    // A card, a menu or a popup over the view takes
                    // the pointer.
                    let reached = hovered
                        .is_none_or(|over| over.contains_key(&node));
                    (relative.cursor_over && reached).then(|| {
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
                    let at = pointer.down_at[index] * pointer.scale;
                    tell(SceneEvent::Pressed {
                        view,
                        button: *which,
                        down: false,
                        at,
                        mods,
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
                held,
                mods,
            });
        }
        pointer.at = Some(at);
        pointer.scale = scale;
        for (index, (button, which)) in BUTTONS.iter().enumerate() {
            if buttons.just_pressed(*button) {
                pressed_any = true;
                focus.0 = Some(entity);
                pointer.down[index] = true;
                pointer.down_at[index] = at;
                tell(SceneEvent::Pressed {
                    view,
                    button: *which,
                    down: true,
                    at: shown,
                    mods,
                });
            } else if pointer.down[index]
                && buttons.just_released(*button)
            {
                pointer.down[index] = false;
                let still = (at - pointer.down_at[index]).length()
                    * scale
                    <= tuning.click_slop;
                if still {
                    pointer.click = Some((*which, mods));
                }
                tell(SceneEvent::Pressed {
                    view,
                    button: *which,
                    down: false,
                    at: shown,
                    mods,
                });
            }
        }
        if turned != Vec2::ZERO {
            tell(SceneEvent::Turned { view, by: turned });
        }
    }
    // A press anywhere else takes the keys from the view pressed
    // last.
    if !pressed_any && buttons.get_just_pressed().next().is_some() {
        focus.0 = None;
    }
}

/// Tells the owner of the view under the pointer, or else of the
/// view pressed last, of each key the shell does not take.
fn type_keys(
    mut typed: MessageReader<KeyboardInput>,
    cameras: Query<(Entity, &Camera, &SceneCamera, &ViewPointer)>,
    focus: Res<ViewFocus>,
    shell: ShellKeys,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mut inputs: MessageWriter<SceneInput>,
) {
    let over = cameras
        .iter()
        .find(|(.., pointer)| pointer.at.is_some())
        .or_else(|| {
            focus
                .0
                .and_then(|focused| cameras.get(focused).ok())
                .filter(|(_, camera, ..)| camera.is_active)
        });
    let mods = Modifiers::held(keys.as_deref());
    for key in typed.read() {
        let Some((_, _, scene_camera, _)) = over else {
            continue;
        };
        let down = key.state == ButtonState::Pressed;
        if key.repeat || (down && shell.takes(key.key_code)) {
            continue;
        }
        inputs.write(SceneInput {
            owner: scene_camera.owner,
            event: SceneEvent::Key {
                view: scene_camera.key,
                key: key.key_code,
                down,
                mods,
            },
        });
    }
}

/// Tells the owner of the view a file was dropped on of it, once the
/// pointer is seen over a view. One that falls on none is let lie.
fn drop_files(
    mut dropped: MessageReader<FileDragAndDrop>,
    cameras: Query<(&SceneCamera, &ViewPointer)>,
    tuning: Res<RenderTuning>,
    time: Res<Time>,
    mut drops: ResMut<Drops>,
    mut inputs: MessageWriter<SceneInput>,
) {
    for drop in dropped.read() {
        if let FileDragAndDrop::DroppedFile { path_buf, .. } = drop {
            drops.0.push((path_buf.clone(), tuning.drop_wait));
        }
    }
    if drops.0.is_empty() {
        return;
    }
    let over = cameras.iter().find_map(|(camera, pointer)| {
        Some((camera, pointer.at? * pointer.scale))
    });
    let Some((camera, at)) = over else {
        let dt = time.delta_secs();
        drops.0.retain_mut(|(_, left)| {
            *left -= dt;
            *left > 0.0
        });
        return;
    };
    for (path, _) in drops.0.drain(..) {
        inputs.write(SceneInput {
            owner: camera.owner,
            event: SceneEvent::Dropped {
                view: camera.key,
                at,
                path,
            },
        });
    }
}
