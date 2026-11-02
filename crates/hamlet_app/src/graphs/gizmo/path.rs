//! Clicks on a spline in the preview: on the curve to add a point
//! there, with Alt on a point to take it away. Each is one step to
//! undo.

use bevy::camera::Camera;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{If, Query, Res, ResMut};
use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
use bevy::math::Vec2;
use bevy::picking::events::{PointerClick, PointerPress};
use bevy::picking::pointer::PointerButton;
use bevy::transform::components::GlobalTransform;
use bevy::ui::{ComputedNode, UiGlobalTransform};
use bevy_fynix::Theme;
use hamlet_blueprint::{Bezier, spline};
use hamlet_graph::{Constant, NodeId, PathPoint};
use hamlet_graph_editor::Editor;
use hamlet_graph_editor::canvas::corner;
use hamlet_ui::theme::HamletTheme;

use super::super::Blueprint;
use super::super::handles::{
    Kind, POINTS, Part, path_of, set, spline_of, targets,
};
use super::super::preview::{Preview, Shown as PreviewFrame};
use super::{Dragging, Handle, Hot, Style};

/// Straight pieces a Bezier is searched along for the point nearest
/// a click.
const STEPS: usize = 48;

/// The furthest the pointer may move between a press and a release
/// that is still a click, in logical pixels.
const SLOP: f32 = 3.0;

/// Where the primary button was last pressed.
#[derive(Resource, Default)]
pub(crate) struct Pressed(Option<Vec2>);

/// Notes where the primary button goes down, so a drag that ends over
/// the curve is not taken for a click.
pub(crate) fn press(
    press: On<PointerPress>,
    mut pressed: ResMut<Pressed>,
) {
    pressed.0 = (press.button == PointerButton::Primary)
        .then_some(press.pointer.position);
}

/// Adds a point to the spline under the pointer, or with Alt held
/// takes away the point under it.
#[expect(
    clippy::too_many_arguments,
    reason = "a click reads the view to find what it is on"
)]
pub(crate) fn click(
    click: On<PointerClick>,
    keys: Res<ButtonInput<KeyCode>>,
    hot: Res<Hot>,
    dragging: Res<Dragging>,
    pressed: Res<Pressed>,
    preview: Option<Res<Preview>>,
    If(theme): If<Res<Theme<HamletTheme>>>,
    frames: Query<
        (&ComputedNode, &UiGlobalTransform),
        With<PreviewFrame>,
    >,
    cameras: Query<(&Camera, &GlobalTransform)>,
    mut editor: ResMut<Editor<Blueprint>>,
) {
    let there = click.pointer.position;
    if click.button != PointerButton::Primary
        || dragging.is_under_way()
        || pressed.0.is_none_or(|start| start.distance(there) > SLOP)
    {
        return;
    }
    let breaks =
        keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
    match hot.0 {
        Some((index, Handle::Centre)) if breaks => {
            let Some(target) =
                targets(&editor).into_iter().nth(index)
            else {
                return;
            };
            let Kind::Knot {
                index,
                part: Part::Anchor,
                ..
            } = target.kind
            else {
                return;
            };
            let Some(mut path) =
                path_of(&editor, target.node, POINTS)
            else {
                return;
            };
            if path.len() > 2 && index < path.len() {
                path.remove(index);
                write(&mut editor, target.node, path);
            }
        }
        Some(_) => {}
        None if breaks => {}
        None => {
            let (Some(preview), Some((frame, at))) =
                (preview, frames.iter().next())
            else {
                return;
            };
            let Ok((camera, eye)) = cameras.get(preview.camera)
            else {
                return;
            };
            let cursor = there - corner(frame, at);
            let reach = Style::of(&theme.0.palette).pick_radius;
            let found = editor
                .selected
                .iter()
                .filter_map(|node| {
                    let spline = spline_of(&editor, *node)?;
                    let (segment, t, distance) = nearest(
                        &spline.points,
                        spline.kind,
                        spline.closed,
                        cursor,
                        |point| {
                            camera.world_to_viewport(eye, point).ok()
                        },
                    )?;
                    Some((distance, *node, segment, t, spline))
                })
                .min_by(|(a, ..), (b, ..)| a.total_cmp(b));
            if let Some((distance, node, segment, t, spline)) = found
                && distance <= reach
            {
                let path = spline::inserted(
                    &spline.points,
                    spline.kind,
                    spline.closed,
                    segment,
                    t,
                );
                write(&mut editor, node, path);
            }
        }
    }
}

/// Sets the points of `node` to `path`, as a step of its own.
fn write(
    editor: &mut Editor<Blueprint>,
    node: NodeId,
    path: Vec<PathPoint>,
) {
    editor.end_step();
    set(editor, node, POINTS, Constant::Path(path));
    editor.end_step();
}

/// The Bezier of the spline through `points` nearest to `cursor`,
/// the parameter on it, and how far from `cursor` that is, where
/// `project` gives where the preview shows a point.
fn nearest(
    points: &[PathPoint],
    kind: spline::SplineKind,
    closed: bool,
    cursor: Vec2,
    project: impl Fn(bevy::math::Vec3) -> Option<Vec2>,
) -> Option<(usize, f32, f32)> {
    let mut best: Option<(usize, f32, f32)> = None;
    for (segment, control) in spline::beziers(points, kind, closed)
        .into_iter()
        .enumerate()
    {
        let bezier = Bezier::new(control);
        let shown = (0..=STEPS)
            .map(|step| {
                project(bezier.point(step as f32 / STEPS as f32))
            })
            .collect::<Option<Vec<_>>>()?;
        for (step, ends) in shown.windows(2).enumerate() {
            let along = ends[1] - ends[0];
            let length = along.length_squared();
            let fraction = if length > f32::EPSILON {
                ((cursor - ends[0]).dot(along) / length)
                    .clamp(0.0, 1.0)
            } else {
                0.0
            };
            let distance =
                cursor.distance(ends[0] + along * fraction);
            if best.is_none_or(|(.., least)| distance < least) {
                let t = (step as f32 + fraction) / STEPS as f32;
                best = Some((segment, t, distance));
            }
        }
    }
    best
}
