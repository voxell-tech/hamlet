//! Handles in the preview for the inputs of the selected cards: a
//! point for a transform's translation, a curve's ends or middle,
//! and a mark on a polygon's rim or a rectangle's sides for its size.
//! A press near one takes hold of it; dragging moves it on the
//! ground, or up and down with Shift, and sets the input as typing it
//! would. An input set from a parameter moves the parameter's
//! default; a wired one has no handle.

use bevy::camera::Camera;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Query, Res, ResMut};
use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
use bevy::math::{Dir3, Vec2, Vec3};
use bevy::picking::events::{
    PointerDrag, PointerDragEnd, PointerDragStart,
};
use bevy::picking::pointer::PointerButton;
use bevy::shape::InfinitePlane3d;
use bevy::transform::components::GlobalTransform;
use bevy::ui::{ComputedNode, UiGlobalTransform};
use hamlet_blueprint::{
    Constant, Does, Edit, Initial, NodeId, Setting,
};

use super::Editor;
use super::canvas::corner;
use super::preview::{Preview, Shown};

/// How near a press must be to a handle on screen to take hold of it,
/// in pixels.
const REACH: f32 = 14.0;

/// One handle: the input of a node it sets, where it is, and how it
/// sets it.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Handle {
    pub(super) node: NodeId,
    pub(super) input: &'static str,
    pub(super) at: Vec3,
    pub(super) kind: Kind,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Kind {
    /// The input is the handle's point.
    Point,
    /// The input is the handle's distance from `middle`, along the
    /// ground, times `times`: a radius, or a side twice as far.
    Distance { middle: Vec3, times: f32 },
}

/// The handle held, and the height its drag on the ground is at.
#[derive(Resource, Default)]
pub(super) struct Held(pub(super) Option<(Handle, f32)>);

/// The handles of every selected card, in the order they are
/// selected.
pub(super) fn handles(editor: &Editor) -> Vec<Handle> {
    let mut found = Vec::new();
    for node in &editor.selected {
        let Some(card) = editor.sheet.graph.node(*node) else {
            continue;
        };
        let Does::Kind(kind) = &card.does else {
            continue;
        };
        let point =
            |input: &'static str| point_of(editor, *node, input);
        let number =
            |input: &'static str| number_of(editor, *node, input);
        let mut add = |input: &'static str, at: Vec3, kind: Kind| {
            found.push(Handle {
                node: *node,
                input,
                at,
                kind,
            });
        };
        match kind.as_str() {
            "transform" => {
                if let Some(at) = point("translation") {
                    add("translation", at, Kind::Point);
                }
            }
            "line" => {
                for end in ["from", "to"] {
                    if let Some(at) = point(end) {
                        add(end, at, Kind::Point);
                    }
                }
            }
            "polygon" => {
                let middle = point("center").unwrap_or(Vec3::ZERO);
                if point("center").is_some() {
                    add("center", middle, Kind::Point);
                }
                if let Some(radius) = number("radius") {
                    let distance =
                        Kind::Distance { middle, times: 1.0 };
                    add(
                        "radius",
                        middle + Vec3::X * radius,
                        distance,
                    );
                }
            }
            "rectangle" => {
                let middle = point("center").unwrap_or(Vec3::ZERO);
                if point("center").is_some() {
                    add("center", middle, Kind::Point);
                }
                let side = Kind::Distance { middle, times: 2.0 };
                if let Some(width) = number("width") {
                    add(
                        "width",
                        middle + Vec3::X * width * 0.5,
                        side,
                    );
                }
                if let Some(depth) = number("depth") {
                    add(
                        "depth",
                        middle + Vec3::Z * depth * 0.5,
                        side,
                    );
                }
            }
            _ => {}
        }
    }
    found
}

/// What an input of `node` is set to, as a constant: fixed, its
/// parameter's default, or its kind's initial value. None for one
/// that is wired, or set otherwise.
fn constant_of(
    editor: &Editor,
    node: NodeId,
    input: &str,
) -> Option<Constant> {
    let graph = &editor.sheet.graph;
    if graph
        .wires
        .iter()
        .any(|wire| wire.to.node == node && wire.to.name == input)
    {
        return None;
    }
    let card = graph.node(node)?;
    match card.settings.get(input) {
        Some(Setting::Fixed(constant)) => Some(constant.clone()),
        Some(Setting::FromParent(name)) => graph
            .inputs
            .iter()
            .find(|parameter| parameter.name == *name)
            .and_then(|parameter| parameter.default.clone()),
        Some(Setting::Random(_)) => None,
        None => {
            let Does::Kind(kind) = &card.does else {
                return None;
            };
            let spec = editor
                .registry
                .get(kind)?
                .inputs
                .iter()
                .find(|spec| spec.name == input)?;
            Some(match spec.initial? {
                Initial::Point(point) => Constant::Point(point),
                Initial::Number(number) => Constant::Number(number),
                Initial::Integer(whole) => Constant::Integer(whole),
                Initial::Bool(flag) => Constant::Bool(flag),
                Initial::Choice(choice) => {
                    Constant::Choice(choice.to_owned())
                }
            })
        }
    }
}

fn point_of(
    editor: &Editor,
    node: NodeId,
    input: &str,
) -> Option<Vec3> {
    match constant_of(editor, node, input)? {
        Constant::Point(point) => Some(point),
        _ => None,
    }
}

fn number_of(
    editor: &Editor,
    node: NodeId,
    input: &str,
) -> Option<f32> {
    match constant_of(editor, node, input)? {
        Constant::Number(number) => Some(number),
        _ => None,
    }
}

/// Sets what `handle` sets to `value`: the input itself, or the
/// default of the parameter it is set from.
fn set(editor: &mut Editor, handle: &Handle, value: Constant) {
    let from = editor
        .sheet
        .graph
        .node(handle.node)
        .and_then(|card| card.settings.get(handle.input))
        .and_then(|setting| match setting {
            Setting::FromParent(name) => Some(name.clone()),
            _ => None,
        });
    let parameter = from.and_then(|name| {
        editor
            .sheet
            .graph
            .inputs
            .iter()
            .find(|parameter| parameter.name == name)
            .cloned()
    });
    match parameter {
        Some(mut parameter) => {
            let name = parameter.name.clone();
            parameter.default = Some(value);
            editor.apply(Edit::ChangeInput {
                name,
                input: parameter,
            });
        }
        None => editor.apply(Edit::Set {
            node: handle.node,
            input: handle.input.to_owned(),
            setting: Some(Setting::Fixed(value)),
        }),
    }
}

/// Where the pointer at `at` in the window is in the preview, in its
/// own pixels from its corner.
fn in_preview(
    at: Vec2,
    frames: &Query<(&ComputedNode, &UiGlobalTransform), With<Shown>>,
) -> Option<Vec2> {
    let (node, transform) = frames.iter().next()?;
    Some(at - corner(node, transform))
}

/// Takes hold of the handle nearest the press, if one is near enough;
/// otherwise the press slides the view.
pub(super) fn grab(
    start: On<PointerDragStart>,
    editor: Res<Editor>,
    preview: Option<Res<Preview>>,
    frames: Query<(&ComputedNode, &UiGlobalTransform), With<Shown>>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    mut held: ResMut<Held>,
) {
    if start.button != PointerButton::Primary {
        return;
    }
    let (Some(preview), Some(at)) =
        (preview, in_preview(start.pointer.position, &frames))
    else {
        return;
    };
    let Ok((camera, eye)) = cameras.get(preview.camera) else {
        return;
    };
    let nearest = handles(&editor)
        .into_iter()
        .filter_map(|handle| {
            let seen =
                camera.world_to_viewport(eye, handle.at).ok()?;
            let off = seen.distance(at);
            (off <= REACH).then_some((handle, off))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1));
    if let Some((handle, _)) = nearest {
        let height = handle.at.y;
        held.0 = Some((handle, height));
    }
}

/// Moves the held handle to where the pointer is: on the ground at
/// its height, or with Shift up and down, facing the camera.
pub(super) fn drag(
    drag: On<PointerDrag>,
    preview: Option<Res<Preview>>,
    frames: Query<(&ComputedNode, &UiGlobalTransform), With<Shown>>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mut held: ResMut<Held>,
    mut editor: ResMut<Editor>,
) {
    let Some((handle, height)) = held.0.clone() else {
        return;
    };
    let (Some(preview), Some(at)) =
        (preview, in_preview(drag.pointer.position, &frames))
    else {
        return;
    };
    let Ok((camera, eye)) = cameras.get(preview.camera) else {
        return;
    };
    let Ok(ray) = camera.viewport_to_world(eye, at) else {
        return;
    };
    let up = keys.is_some_and(|keys| {
        keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight])
    }) && handle.kind == Kind::Point;
    let plane = if up {
        // Upright, facing the camera across the ground.
        let facing = (eye.translation() - handle.at)
            * Vec3::new(1.0, 0.0, 1.0);
        let normal = Dir3::new(facing).unwrap_or(Dir3::Z);
        (handle.at, InfinitePlane3d::new(normal))
    } else {
        (Vec3::new(0.0, height, 0.0), InfinitePlane3d::new(Dir3::Y))
    };
    let Some(distance) = ray.intersect_plane(plane.0, plane.1) else {
        return;
    };
    let hit = ray.get_point(distance);
    let moved = match handle.kind {
        Kind::Point if up => {
            Vec3::new(handle.at.x, hit.y, handle.at.z)
        }
        Kind::Point => Vec3::new(hit.x, handle.at.y, hit.z),
        Kind::Distance { .. } => hit,
    };
    let value = match handle.kind {
        Kind::Point => Constant::Point(round(moved)),
        Kind::Distance { middle, times } => {
            let along = (moved - middle) * Vec3::new(1.0, 0.0, 1.0);
            Constant::Number(round_one(along.length() * times))
        }
    };
    set(&mut editor, &handle, value);
    let mut handle = handle;
    if handle.kind == Kind::Point {
        handle.at = moved;
    }
    held.0 = Some((handle, height));
}

/// Lets go of the handle.
pub(super) fn release(_: On<PointerDragEnd>, mut held: ResMut<Held>) {
    held.0 = None;
}

/// A point to the centimetre, so dragged values stay readable.
fn round(point: Vec3) -> Vec3 {
    (point * 100.0).round() / 100.0
}

fn round_one(value: f32) -> f32 {
    (value * 100.0).round() / 100.0
}
