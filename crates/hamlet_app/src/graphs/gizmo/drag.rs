//! Dragging a gizmo's handle: the input it sets written as the
//! pointer moves, undone as one, and put back by Escape.

use bevy::camera::Camera;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{If, Query, Res, ResMut};
use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
use bevy::math::{Quat, Vec2, Vec3};
use bevy::picking::events::{
    PointerDrag, PointerDragEnd, PointerDragStart,
};
use bevy::picking::pointer::PointerButton;
use bevy::shape::Ray3d;
use bevy::transform::components::GlobalTransform;
use bevy::ui::{ComputedNode, UiGlobalTransform};
use bevy_fynix::Theme;
use hamlet_graph::{Constant, NodeId};
use hamlet_graph_editor::Editor;
use hamlet_graph_editor::canvas::corner;
use hamlet_ui::theme::HamletTheme;

use super::super::Blueprint;
use super::super::handles::{
    Kind, POINTS, Part, Target, degrees_of, path_of, set, targets,
};
use super::super::preview::{Preview, Shown as PreviewFrame};
use super::{Arc, Frame, GizmoMode, Handle, Hot, Style, handles_of};

/// The least a scale drag scales by.
const MIN_FACTOR: f32 = 1e-3;

/// The pointer over the preview: where it is and the ray it casts.
#[derive(Clone, Copy, Debug)]
pub(super) struct Pointing {
    pub(super) cursor: Vec2,
    pub(super) ray: Ray3d,
}

/// A handle as it was when a drag took hold of it.
#[derive(Clone, Copy, Debug)]
pub(super) struct Grab {
    pub(super) handle: Handle,
    pub(super) frame: Frame,
    pub(super) pointing: Pointing,
    /// The origin, where the preview shows it.
    pub(super) centre: Vec2,
    /// A handle's length there.
    pub(super) reach: f32,
}

/// How far along the line through `origin` in the unit direction
/// `axis` the point nearest to `ray` is. `None` when the two are
/// parallel.
fn along_axis(ray: Ray3d, origin: Vec3, axis: Vec3) -> Option<f32> {
    let direction = *ray.direction;
    let between = origin - ray.origin;
    let lean = axis.dot(direction);
    let denominator = 1.0 - lean * lean;
    if denominator <= 1e-6 {
        return None;
    }
    Some(
        (lean * direction.dot(between) - axis.dot(between))
            / denominator,
    )
}

/// Where `ray` meets the plane through `origin` facing `normal`.
fn plane_hit(ray: Ray3d, origin: Vec3, normal: Vec3) -> Option<Vec3> {
    let denominator = normal.dot(*ray.direction);
    if denominator.abs() <= 1e-6 {
        return None;
    }
    let distance = (origin - ray.origin).dot(normal) / denominator;
    Some(ray.origin + *ray.direction * distance)
}

/// `value` to the nearest multiple of `step`, when there is one.
fn snapped(value: f32, step: Option<f32>) -> f32 {
    match step {
        Some(step) if step > 0.0 => (value / step).round() * step,
        _ => value,
    }
}

impl Grab {
    /// How far the drag has moved the target, in the world.
    fn translation(
        &self,
        now: &Pointing,
        snap: Option<f32>,
    ) -> Option<Vec3> {
        let Frame { origin, axes, .. } = self.frame;
        match self.handle {
            Handle::Axis(axis) => {
                let axis = axes[axis];
                let from =
                    along_axis(self.pointing.ray, origin, axis)?;
                let to = along_axis(now.ray, origin, axis)?;
                Some(axis * snapped(to - from, snap))
            }
            Handle::Plane(axis) => {
                let normal = axes[axis];
                let from =
                    plane_hit(self.pointing.ray, origin, normal)?;
                let to = plane_hit(now.ray, origin, normal)?;
                let [a, b] = self.frame.sides(axis);
                let along =
                    |side: Vec3| snapped((to - from).dot(side), snap);
                Some(a * along(a) + b * along(b))
            }
            Handle::Centre => {
                let normal = self.frame.facing * Vec3::Z;
                let from =
                    plane_hit(self.pointing.ray, origin, normal)?;
                let to = plane_hit(now.ray, origin, normal)?;
                Some((to - from).map(|part| snapped(part, snap)))
            }
            Handle::View | Handle::Ball => None,
        }
    }

    /// The axis the drag has turned the target about and the angle it
    /// has turned it by, in the world.
    fn turn(
        &self,
        now: &Pointing,
        snap: Option<f32>,
    ) -> Option<(Vec3, f32)> {
        let axis = match self.handle {
            Handle::Axis(axis) => self.frame.axes[axis],
            Handle::View => -self.frame.look,
            Handle::Ball => {
                let rolled = now.cursor - self.pointing.cursor;
                // The way the near side of a ball goes under the
                // pointer, a handle's length of it turning a radian.
                let axis = self.frame.facing
                    * Vec3::new(rolled.y, rolled.x, 0.0);
                let angle = rolled.length() / self.reach;
                return Some((
                    axis.try_normalize()?,
                    snapped(angle, snap),
                ));
            }
            Handle::Plane(_) | Handle::Centre => return None,
        };
        let from = self.pointing.cursor - self.centre;
        let to = now.cursor - self.centre;
        // Clockwise as drawn, the preview's y running down.
        let swept = from.perp_dot(to).atan2(from.dot(to));
        // A turn about an axis is anticlockwise seen from its tip.
        let toward_viewer = axis.dot(self.frame.look) < 0.0;
        let angle = if toward_viewer { -swept } else { swept };
        Some((axis, snapped(angle, snap)))
    }

    /// The factor the drag has scaled each of the target's axes by.
    fn scale(
        &self,
        now: &Pointing,
        snap: Option<f32>,
    ) -> Option<Vec3> {
        let Frame { origin, axes, .. } = self.frame;
        // A target scaled to zero has no inverse, and no later drag
        // scales it back.
        let factor = |raw: f32| snapped(raw, snap).max(MIN_FACTOR);
        let spread = || {
            let from = self.pointing.cursor.distance(self.centre);
            let to = now.cursor.distance(self.centre);
            factor(1.0 + (to - from) / self.reach)
        };
        match self.handle {
            Handle::Axis(axis) => {
                let from = along_axis(
                    self.pointing.ray,
                    origin,
                    axes[axis],
                )?;
                let to = along_axis(now.ray, origin, axes[axis])?;
                let length = self.reach * self.frame.scale;
                let mut scale = Vec3::ONE;
                scale[axis] = factor(1.0 + (to - from) / length);
                Some(scale)
            }
            Handle::Plane(axis) => {
                let mut scale = Vec3::splat(spread());
                scale[axis] = 1.0;
                Some(scale)
            }
            Handle::Centre => Some(Vec3::splat(spread())),
            Handle::View | Handle::Ball => None,
        }
    }

    /// The arc a turn by `angle` has swept, and the radius of the
    /// ring it is on.
    pub(super) fn sweep(
        &self,
        angle: f32,
        style: &Style,
    ) -> Option<(Arc, f32)> {
        let Frame { origin, look, .. } = self.frame;
        let (normal, radius) = match self.handle {
            Handle::Axis(axis) => (self.frame.axes[axis], style.size),
            Handle::View => (-look, style.outer_ring),
            _ => return None,
        };
        // From where the pointer took hold of the ring's plane, or
        // from the nearest of the ring when that is edge on.
        let start = plane_hit(self.pointing.ray, origin, normal)
            .and_then(|hit| (hit - origin).try_normalize())
            .unwrap_or_else(|| Arc::facing(normal, look, 0.0).start);
        let arc = Arc {
            start,
            quarter: normal.cross(start),
            from: 0.0,
            to: angle,
        };
        Some((arc, radius))
    }
}

/// The drag under way: of which target, how it began, and what it
/// writes.
pub(super) struct Drag {
    pub(super) index: usize,
    target: Target,
    mode: GizmoMode,
    pub(super) grab: Grab,
    /// The angle a turn has turned it by.
    pub(super) angle: f32,
    /// The input written, and what it was, for Escape to put back.
    written: (NodeId, &'static str, Constant),
}

/// The gizmo drag under way, of which there is one at most.
#[derive(Resource, Default)]
pub(crate) struct Dragging(pub(super) Option<Drag>);

impl Dragging {
    pub(crate) fn is_under_way(&self) -> bool {
        self.0.is_some()
    }
}

/// The pointer at `at` in the window, over the preview, through its
/// camera.
pub(super) fn pointing(
    at: Vec2,
    frames: &Query<
        (&ComputedNode, &UiGlobalTransform),
        With<PreviewFrame>,
    >,
    camera: &Camera,
    eye: &GlobalTransform,
) -> Option<Pointing> {
    let (node, transform) = frames.iter().next()?;
    let cursor = at - corner(node, transform);
    let ray = camera.viewport_to_world(eye, cursor).ok()?;
    Some(Pointing { cursor, ray })
}

/// What a drag of `target` in `mode` writes, and its value now.
fn written(
    editor: &Editor<Blueprint>,
    target: &Target,
    mode: GizmoMode,
) -> Option<(&'static str, Constant)> {
    Some(match &target.kind {
        Kind::Knot { .. } => (
            POINTS,
            Constant::Path(path_of(editor, target.node, POINTS)?),
        ),
        Kind::Transform { rotation, scale } => match mode {
            GizmoMode::Translate => {
                ("translation", Constant::Point(target.at))
            }
            GizmoMode::Rotate => {
                ("rotation", Constant::Point(degrees_of(*rotation)))
            }
            GizmoMode::Scale => ("scale", Constant::Point(*scale)),
        },
        Kind::Point { input } => (input, Constant::Point(target.at)),
        Kind::Distance {
            input,
            middle,
            axis,
            times,
        } => (
            input,
            Constant::Number(
                (target.at - *middle).dot(*axis) * times,
            ),
        ),
    })
}

/// Takes hold of the handle under the pointer, if there is one; the
/// press then moves the handle, not the view.
#[expect(
    clippy::too_many_arguments,
    reason = "a drag reads all it takes hold of"
)]
pub(crate) fn grab(
    start: On<PointerDragStart>,
    mut editor: ResMut<Editor<Blueprint>>,
    mode: Res<GizmoMode>,
    hot: Res<Hot>,
    preview: Option<Res<Preview>>,
    If(theme): If<Res<Theme<HamletTheme>>>,
    frames: Query<
        (&ComputedNode, &UiGlobalTransform),
        With<PreviewFrame>,
    >,
    cameras: Query<(&Camera, &GlobalTransform)>,
    mut dragging: ResMut<Dragging>,
) {
    if start.button != PointerButton::Primary {
        return;
    }
    let (Some((index, handle)), Some(preview)) = (hot.0, preview)
    else {
        return;
    };
    let Some(target) = targets(&editor).into_iter().nth(index) else {
        return;
    };
    let Ok((camera, eye)) = cameras.get(preview.camera) else {
        return;
    };
    let (handles, turn) = handles_of(&target, *mode);
    let Some(frame) = Frame::of(target.at, turn, camera, eye) else {
        return;
    };
    let (Some(now), Ok(centre)) = (
        pointing(start.pointer.position, &frames, camera, eye),
        camera.world_to_viewport(eye, frame.origin),
    ) else {
        return;
    };
    let Some((input, was)) = written(&editor, &target, handles.mode)
    else {
        return;
    };
    // The drag is one step to undo.
    editor.end_step();
    dragging.0 = Some(Drag {
        index,
        written: (target.node, input, was),
        mode: handles.mode,
        grab: Grab {
            handle,
            frame,
            pointing: now,
            centre,
            reach: Style::of(&theme.0.palette).size,
        },
        target,
        angle: 0.0,
    });
}

/// Writes the drag under way as the pointer moves. Ctrl snaps it.
#[expect(
    clippy::too_many_arguments,
    reason = "a drag reads all it writes from"
)]
pub(crate) fn steer(
    drag: On<PointerDrag>,
    preview: Option<Res<Preview>>,
    keys: Res<ButtonInput<KeyCode>>,
    If(theme): If<Res<Theme<HamletTheme>>>,
    frames: Query<
        (&ComputedNode, &UiGlobalTransform),
        With<PreviewFrame>,
    >,
    cameras: Query<(&Camera, &GlobalTransform)>,
    mut dragging: ResMut<Dragging>,
    mut editor: ResMut<Editor<Blueprint>>,
) {
    let (Some(held), Some(preview)) = (dragging.0.as_mut(), preview)
    else {
        return;
    };
    let Ok((camera, eye)) = cameras.get(preview.camera) else {
        return;
    };
    let Some(now) =
        pointing(drag.pointer.position, &frames, camera, eye)
    else {
        return;
    };
    let style = Style::of(&theme.0.palette);
    let snaps = keys
        .any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let (node, input, _) = held.written.clone();
    let value = match (&held.target.kind, held.mode) {
        (Kind::Transform { rotation, .. }, GizmoMode::Rotate) => {
            let Some((axis, angle)) = held
                .grab
                .turn(&now, snaps.then_some(style.rotate_snap))
            else {
                return;
            };
            held.angle = angle;
            let turned =
                Quat::from_axis_angle(axis, angle) * *rotation;
            Constant::Point(round(degrees_of(turned.normalize())))
        }
        (Kind::Transform { scale, .. }, GizmoMode::Scale) => {
            let Some(factor) = held
                .grab
                .scale(&now, snaps.then_some(style.scale_snap))
            else {
                return;
            };
            Constant::Point(round(*scale * factor))
        }
        (
            Kind::Distance {
                middle,
                axis,
                times,
                ..
            },
            _,
        ) => {
            let Some(delta) = held.grab.translation(
                &now,
                snaps.then_some(style.translate_snap),
            ) else {
                return;
            };
            let length =
                (held.target.at + delta - *middle).dot(*axis) * times;
            Constant::Number(
                (length.max(0.01) * 100.0).round() / 100.0,
            )
        }
        (
            Kind::Knot {
                index,
                part,
                anchor,
            },
            _,
        ) => {
            let Some(delta) = held.grab.translation(
                &now,
                snaps.then_some(style.translate_snap),
            ) else {
                return;
            };
            let Some(mut path) = path_of(&editor, node, input) else {
                return;
            };
            let Some(point) = path.get_mut(*index) else {
                return;
            };
            let moved = round(held.target.at + delta);
            // Alt breaks the symmetry; otherwise the other handle
            // mirrors this one.
            let mirrors = !keys
                .any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
            match part {
                Part::Anchor => point.at = moved,
                Part::After => {
                    point.after = round(moved - *anchor);
                    if mirrors {
                        point.before = -point.after;
                    }
                }
                Part::Before => {
                    point.before = round(moved - *anchor);
                    if mirrors {
                        point.after = -point.before;
                    }
                }
            }
            Constant::Path(path)
        }
        _ => {
            let Some(delta) = held.grab.translation(
                &now,
                snaps.then_some(style.translate_snap),
            ) else {
                return;
            };
            Constant::Point(round(held.target.at + delta))
        }
    };
    set(&mut editor, node, input, value);
}

/// Lets go of the handle, keeping what it set.
pub(crate) fn release(
    end: On<PointerDragEnd>,
    mut dragging: ResMut<Dragging>,
    mut editor: ResMut<Editor<Blueprint>>,
) {
    if end.button == PointerButton::Primary && dragging.0.is_some() {
        dragging.0 = None;
        editor.end_step();
    }
}

/// Lets Escape take back the drag under way, putting the input back.
pub(crate) fn cancel(
    keys: Res<ButtonInput<KeyCode>>,
    mut dragging: ResMut<Dragging>,
    mut editor: ResMut<Editor<Blueprint>>,
) {
    if dragging.0.is_none() || !keys.just_pressed(KeyCode::Escape) {
        return;
    }
    if let Some(drag) = dragging.0.take() {
        let (node, input, was) = drag.written;
        set(&mut editor, node, input, was);
        editor.end_step();
    }
}

/// A point to the centimetre, so dragged values stay readable.
fn round(point: Vec3) -> Vec3 {
    (point * 100.0).round() / 100.0
}
