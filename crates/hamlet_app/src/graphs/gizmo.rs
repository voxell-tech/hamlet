//! The gizmo in the preview, on what the selected cards offer:
//! arrows, squares and a centre to move, rings to turn, and boxes to
//! scale, drawn the same size on screen however near, each fading as
//! it comes to point at the viewer, the one under the pointer
//! brighter. A transform node takes all three, G, R and S picking
//! which; a point only moves; a radius or a side is one arrow along
//! it. A spline's points are small rings to move, and its tangent
//! handles rings joined to their points by a hair; clicking the curve
//! adds a point and Alt-clicking a point takes it away.

mod drag;
pub(super) mod fills;
mod paint;
mod path;

use core::f32::consts::PI;

use bevy::app::{App, PostUpdate, Startup, Update};
use bevy::asset::Assets;
use bevy::camera::Camera;
use bevy::camera::visibility::{RenderLayers, Visibility};
use bevy::color::Color;
use bevy::ecs::change_detection::DetectChangesMut;
use bevy::ecs::query::{Changed, With};
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{If, Local, Query, Res, ResMut, Single};
use bevy::gizmos::AppGizmoBuilder;
use bevy::gizmos::config::{GizmoConfigGroup, GizmoConfigStore};
use bevy::gizmos::gizmos::Gizmos;
use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
use bevy::input::mouse::MouseButton;
use bevy::math::{Quat, Vec2, Vec3};
use bevy::mesh::{Mesh, Mesh3d};
use bevy::pbr::MaterialPlugin;
use bevy::reflect::Reflect;
use bevy::transform::components::GlobalTransform;
use bevy::ui::{ComputedNode, UiGlobalTransform};
use bevy::window::{PrimaryWindow, SystemCursorIcon, Window};
use bevy_fynix::{OverrideCursor, Theme};
use hamlet_graph_editor::Editor;
use hamlet_graph_editor::canvas::corner;
use hamlet_realm::sky::OwnSky;
use hamlet_ui::theme::{HamletTheme, Palette};

use self::drag::cancel;
pub(super) use self::drag::{Dragging, grab, release, steer};
use self::fills::{FillMaterial, Fills, HandleFills, spawn_fills};
use self::paint::{Shown, paint};
use self::path::Pressed;
pub(super) use self::path::{click, press};
use super::Blueprint;
use super::handles::{Kind, Part, Target, targets};
use super::preview::{LAYER, Preview, Shown as PreviewFrame};

/// Segments a rotation ring is drawn and picked by.
const RING_STEPS: usize = 48;

pub(super) fn plugin(app: &mut App) {
    app.init_gizmo_group::<HandleGizmos>()
        .add_plugins(MaterialPlugin::<FillMaterial>::default())
        .init_resource::<GizmoMode>()
        .init_resource::<Hot>()
        .init_resource::<Dragging>()
        .init_resource::<Pressed>()
        .init_resource::<Background>()
        .add_systems(Startup, (style_handles, spawn_fills))
        .add_systems(
            Update,
            (pick_mode, cancel, grab_cursor, read_background),
        )
        .add_systems(PostUpdate, handles);
}

/// Asks for the open hand while the pointer is on a handle and the
/// closed one while a handle is dragged. It lets go of the cursor
/// only when it was the one that asked for it.
fn grab_cursor(
    hot: Res<Hot>,
    dragging: Res<Dragging>,
    mut cursor: ResMut<OverrideCursor>,
    mut asked: Local<Option<SystemCursorIcon>>,
) {
    let want = if dragging.0.is_some() {
        Some(SystemCursorIcon::Grabbing)
    } else if hot.0.is_some() {
        Some(SystemCursorIcon::Grab)
    } else {
        None
    };
    if want == *asked {
        return;
    }
    if want.is_some() {
        cursor.0 = want;
    } else if cursor.0 == *asked {
        cursor.0 = None;
    }
    *asked = want;
}

/// The gizmo's lines, seen only by the preview's camera.
#[derive(Default, Reflect, GizmoConfigGroup)]
pub(super) struct HandleGizmos;

fn style_handles(mut store: ResMut<GizmoConfigStore>) {
    let (config, _) = store.config_mut::<HandleGizmos>();
    config.render_layers = RenderLayers::layer(LAYER);
    config.line.width = 3.0;
    // In front of whatever the handles sit inside.
    config.depth_bias = -1.0;
}

/// What the gizmo of a transform node edits.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum GizmoMode {
    #[default]
    Translate,
    Rotate,
    Scale,
}

/// Picks the gizmo's mode with G, R and S while nothing is typed in.
/// A key held with a modifier is a command's, as Cmd+S is saving and
/// Shift+R the preview's right view.
fn pick_mode(
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<bevy::input_focus::InputFocus>,
    dragging: Res<Dragging>,
    mut mode: ResMut<GizmoMode>,
) {
    let modified = keys.any_pressed([
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::AltLeft,
        KeyCode::AltRight,
        KeyCode::ShiftLeft,
        KeyCode::ShiftRight,
    ]);
    if focus.get().is_some() || dragging.0.is_some() || modified {
        return;
    }
    let picked = if keys.just_pressed(KeyCode::KeyG) {
        GizmoMode::Translate
    } else if keys.just_pressed(KeyCode::KeyR) {
        GizmoMode::Rotate
    } else if keys.just_pressed(KeyCode::KeyS) {
        GizmoMode::Scale
    } else {
        return;
    };
    if *mode != picked {
        *mode = picked;
    }
}

/// Lengths in logical pixels on screen, and the gizmo's colours.
#[derive(Clone, Copy, Debug)]
pub(super) struct Style {
    pub(super) size: f32,
    pub(super) centre: f32,
    pub(super) outer_ring: f32,
    pub(super) cone_length: f32,
    pub(super) cone_radius: f32,
    pub(super) tip: f32,
    pub(super) plane_offset: f32,
    pub(super) plane_size: f32,
    pub(super) pick_radius: f32,
    pub(super) ring_overlap: f32,
    pub(super) axis_fade: [f32; 2],
    pub(super) plane_fade: [f32; 2],
    pub(super) rest_alpha: f32,
    pub(super) hot_alpha: f32,
    pub(super) plane_fill: f32,
    pub(super) ball_alpha: f32,
    /// Metres a snapped move goes in.
    pub(super) translate_snap: f32,
    /// Radians a snapped turn turns in.
    pub(super) rotate_snap: f32,
    pub(super) scale_snap: f32,
    pub(super) axes: [Color; 3],
    pub(super) centre_color: Color,
    pub(super) sweep: Color,
    /// The radius of a spline point's ring.
    pub(super) knot: f32,
    /// The radius of a tangent handle's ring.
    pub(super) tangent: f32,
    /// Half the width of the line to a tangent handle.
    pub(super) hair: f32,
}

impl Style {
    pub(super) fn of(palette: &Palette) -> Self {
        use bevy::color::Alpha;
        Self {
            size: 90.0,
            centre: 18.0,
            outer_ring: 108.0,
            cone_length: 22.0,
            cone_radius: 5.5,
            tip: 9.0,
            plane_offset: 51.0,
            plane_size: 13.0,
            pick_radius: 8.0,
            ring_overlap: 0.02,
            axis_fade: [0.02, 0.1],
            plane_fade: [0.175, 0.25],
            rest_alpha: 0.6,
            hot_alpha: 1.0,
            plane_fill: 0.5,
            ball_alpha: 0.05,
            translate_snap: 0.1,
            rotate_snap: PI / 12.0,
            scale_snap: 0.1,
            axes: [palette.red, palette.green, palette.agent],
            centre_color: palette.gold_1,
            sweep: palette.gold_1.with_alpha(0.2),
            knot: 7.0,
            tangent: 5.0,
            hair: 0.75,
        }
    }

    /// This, its colours set against `background`: as made for a
    /// dark one, and darker and more opaque on a light one.
    pub(super) fn against(
        self,
        palette: &Palette,
        background: Background,
    ) -> Self {
        use bevy::color::{Alpha, Luminance};
        match background {
            Background::Dark => self,
            Background::Light => Self {
                axes: self
                    .axes
                    .map(|colour| colour.darker(LIGHT_DARKEN)),
                centre_color: palette.input,
                sweep: palette.input.with_alpha(0.2),
                rest_alpha: 0.9,
                ..self
            },
        }
    }
}

/// How much darker the axes are drawn on a light background.
pub(super) const LIGHT_DARKEN: f32 = 0.2;

/// The haze's luminance above which the preview's background counts
/// as light.
const LIGHT_HAZE: f32 = 0.45;

/// Whether the preview's background is light or dark, from the haze
/// of its sky, so the gizmo can be seen on either.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Default, Debug)]
pub(super) enum Background {
    Light,
    #[default]
    Dark,
}

/// Sets [`Background`] from the preview camera's sky as it changes.
fn read_background(
    preview: Option<Res<Preview>>,
    skies: Query<&OwnSky, Changed<OwnSky>>,
    mut background: ResMut<Background>,
) {
    use bevy::color::Luminance;
    let Some(sky) =
        preview.and_then(|preview| skies.get(preview.camera).ok())
    else {
        return;
    };
    let now = if sky.0.look.haze.luminance() > LIGHT_HAZE {
        Background::Light
    } else {
        Background::Dark
    };
    if *background != now {
        *background = now;
    }
}

/// One handle of a gizmo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Handle {
    /// Along or around the axis of this index.
    Axis(usize),
    /// Along the two axes the axis of this index is square to.
    Plane(usize),
    /// At the origin: across the view for a move, every axis at once
    /// for a scale.
    Centre,
    /// Around the line of sight.
    View,
    /// Any way round, as a ball under the pointer rolls.
    Ball,
}

/// The handle under the preview's pointer: of which target, and
/// which of its handles.
#[derive(Resource, Default)]
pub(super) struct Hot(pub(super) Option<(usize, Handle)>);

/// An arc of a unit circle, from the angle `from` to the angle `to`.
#[derive(Clone, Copy, Debug)]
pub(super) struct Arc {
    /// The way to the point at no angle.
    pub(super) start: Vec3,
    /// The way to the point a quarter turn on.
    pub(super) quarter: Vec3,
    pub(super) from: f32,
    pub(super) to: f32,
}

impl Arc {
    /// The whole circle around `normal`.
    pub(super) fn full(normal: Vec3) -> Self {
        let start = normal.any_orthonormal_vector();
        Self {
            start,
            quarter: normal.cross(start),
            from: -PI,
            to: PI,
        }
    }

    /// The half of the circle around `normal` nearer to a viewer
    /// looking along `look`, and as much of the rest as is within
    /// `overlap` behind the centre.
    pub(super) fn facing(
        normal: Vec3,
        look: Vec3,
        overlap: f32,
    ) -> Self {
        let away = look - normal * normal.dot(look);
        let lean = away.length();
        if lean <= overlap.max(f32::EPSILON) {
            return Self::full(normal);
        }
        let start = -away / lean;
        let half = (-overlap / lean).acos();
        Self {
            start,
            quarter: normal.cross(start),
            from: -half,
            to: half,
        }
    }

    fn at(&self, angle: f32) -> Vec3 {
        self.start * angle.cos() + self.quarter * angle.sin()
    }

    /// The ends of `steps` segments along it, `radius` from `centre`.
    pub(super) fn points(
        self,
        centre: Vec3,
        radius: f32,
        steps: usize,
    ) -> impl Iterator<Item = Vec3> {
        (0..=steps).map(move |step| {
            let along = step as f32 / steps as f32;
            let angle = self.from + (self.to - self.from) * along;
            centre + self.at(angle) * radius
        })
    }
}

/// How much of a handle shows: none of it with `lean` at `gone` or
/// under, all of it from `full` up.
fn fade(lean: f32, [gone, full]: [f32; 2]) -> f32 {
    ((lean - gone) / (full - gone)).clamp(0.0, 1.0)
}

/// Where a gizmo sits and how large it is, in the world.
#[derive(Clone, Copy, Debug)]
pub(super) struct Frame {
    pub(super) origin: Vec3,
    pub(super) axes: [Vec3; 3],
    /// The length of a logical pixel at the origin.
    pub(super) scale: f32,
    /// The way the viewer looks at the origin.
    pub(super) look: Vec3,
    /// The rotation of the camera.
    pub(super) facing: Quat,
}

impl Frame {
    /// The gizmo at `origin`, its axes turned by `rotation`, as
    /// `camera` draws it.
    pub(super) fn of(
        origin: Vec3,
        rotation: Quat,
        camera: &Camera,
        view: &GlobalTransform,
    ) -> Option<Self> {
        let at = camera.world_to_viewport(view, origin).ok()?;
        let look =
            *camera.viewport_to_world(view, at).ok()?.direction;
        let beside = camera
            .world_to_viewport(
                view,
                origin + view.rotation() * Vec3::X,
            )
            .ok()?;
        let per_unit = at.distance(beside);
        if per_unit <= f32::EPSILON {
            return None;
        }
        Some(Self {
            origin,
            axes: Vec3::AXES.map(|axis| rotation * axis),
            scale: 1.0 / per_unit,
            look,
            facing: view.rotation(),
        })
    }

    /// The point `pixels` along `axis` from the origin.
    pub(super) fn along(&self, axis: usize, pixels: f32) -> Vec3 {
        self.origin + self.axes[axis] * pixels * self.scale
    }

    /// The two axes the plane handle of `axis` lies along.
    pub(super) fn sides(&self, axis: usize) -> [Vec3; 2] {
        [self.axes[(axis + 1) % 3], self.axes[(axis + 2) % 3]]
    }

    /// The corners of the plane handle of `axis`, in turn.
    pub(super) fn plane(
        &self,
        axis: usize,
        style: &Style,
    ) -> [Vec3; 4] {
        let [a, b] = self.sides(axis);
        let middle =
            self.origin + (a + b) * style.plane_offset * self.scale;
        let half = style.plane_size / 2.0 * self.scale;
        [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
            .map(|(x, y)| middle + (a * x + b * y) * half)
    }

    /// The points of the circle `arc` is of, `pixels` from the
    /// origin.
    pub(super) fn ring(
        &self,
        arc: Arc,
        pixels: f32,
    ) -> impl Iterator<Item = Vec3> {
        arc.points(self.origin, pixels * self.scale, RING_STEPS)
    }

    /// The part of the rotation ring of `axis` that is drawn.
    pub(super) fn arc(&self, axis: usize, style: &Style) -> Arc {
        Arc::facing(self.axes[axis], self.look, style.ring_overlap)
    }

    /// How much of the handle along `axis` shows, which is less as it
    /// comes to point at the viewer.
    pub(super) fn axis_fade(
        &self,
        axis: usize,
        style: &Style,
    ) -> f32 {
        let lean = 1.0 - self.axes[axis].dot(self.look).abs();
        fade(lean, style.axis_fade)
    }

    /// How much of the plane handle of `axis` shows, which is less as
    /// it comes to be seen edge on.
    pub(super) fn plane_fade(
        &self,
        axis: usize,
        style: &Style,
    ) -> f32 {
        fade(self.axes[axis].dot(self.look).abs(), style.plane_fade)
    }
}

/// How much of a gizmo a target shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Shape {
    /// Its arrows, rings or boxes.
    Axes,
    /// A small ring to move a point by.
    Knot,
    /// A smaller ring to move the end of a handle by, joined to the
    /// point it belongs to at `anchor`.
    Tangent { anchor: Vec3 },
}

impl Shape {
    /// The radius of its ring in logical pixels.
    fn ring(self, style: &Style) -> f32 {
        match self {
            Shape::Tangent { .. } => style.tangent,
            Shape::Axes | Shape::Knot => style.knot,
        }
    }
}

/// The handles of a target's gizmo: what it edits, and, for a
/// distance, the one axis it pulls along.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Handles {
    pub(super) mode: GizmoMode,
    pub(super) only: Option<usize>,
    pub(super) shape: Shape,
}

/// The [`Handles`] of `target`'s gizmo in `mode`, and the turn of its
/// axes: a transform node's own turn when it scales, the world's
/// otherwise.
fn handles_of(target: &Target, mode: GizmoMode) -> (Handles, Quat) {
    match &target.kind {
        Kind::Transform { rotation, .. } => {
            let turn = if mode == GizmoMode::Scale {
                *rotation
            } else {
                Quat::IDENTITY
            };
            (
                Handles {
                    mode,
                    only: None,
                    shape: Shape::Axes,
                },
                turn,
            )
        }
        Kind::Point { .. } => (
            Handles {
                mode: GizmoMode::Translate,
                only: None,
                shape: Shape::Axes,
            },
            Quat::IDENTITY,
        ),
        Kind::Knot { part, anchor, .. } => (
            Handles {
                mode: GizmoMode::Translate,
                only: None,
                shape: match part {
                    Part::Anchor => Shape::Knot,
                    Part::Before | Part::After => {
                        Shape::Tangent { anchor: *anchor }
                    }
                },
            },
            Quat::IDENTITY,
        ),
        Kind::Distance { axis, .. } => {
            let index =
                if axis.z.abs() > axis.x.abs() { 2 } else { 0 };
            (
                Handles {
                    mode: GizmoMode::Translate,
                    only: Some(index),
                    shape: Shape::Axes,
                },
                Quat::IDENTITY,
            )
        }
    }
}

/// Whether `point` is inside the convex outline through `corners`.
fn is_inside(point: Vec2, corners: &[Vec2]) -> bool {
    let turns = (0..corners.len()).map(|at| {
        let from = corners[at];
        let to = corners[(at + 1) % corners.len()];
        (to - from).perp_dot(point - from)
    });
    turns.clone().all(|turn| turn >= 0.0)
        || turns.clone().all(|turn| turn <= 0.0)
}

/// The distance from `point` to the segment from `a` to `b`.
fn segment_distance(point: Vec2, a: Vec2, b: Vec2) -> f32 {
    let along = b - a;
    let length = along.length_squared();
    if length <= f32::EPSILON {
        return point.distance(a);
    }
    let fraction = ((point - a).dot(along) / length).clamp(0.0, 1.0);
    point.distance(a + along * fraction)
}

/// The handle of the axis nearest by `distance`, of those within
/// `reach`.
fn nearest_axis(
    reach: f32,
    distance: impl Fn(usize) -> Option<f32>,
) -> Option<(Handle, f32)> {
    (0..3)
        .filter_map(|axis| Some((axis, distance(axis)?)))
        .filter(|(_, distance)| *distance <= reach)
        .min_by(|(_, a), (_, b)| a.total_cmp(b))
        .map(|(axis, distance)| (Handle::Axis(axis), distance))
}

/// The handle of `frame` under `cursor` and how near it is,
/// `project` taking a point in the world to where the preview shows
/// it.
fn pick(
    handles: Handles,
    frame: &Frame,
    cursor: Vec2,
    style: &Style,
    project: impl Fn(Vec3) -> Option<Vec2>,
) -> Option<(Handle, f32)> {
    let from_centre = cursor.distance(project(frame.origin)?);
    if handles.shape != Shape::Axes {
        let reach = handles.shape.ring(style).max(style.pick_radius);
        return (from_centre <= reach)
            .then_some((Handle::Centre, from_centre));
    }
    let arrow = |axis: usize| {
        if frame.axis_fade(axis, style) <= 0.0 {
            return None;
        }
        Some(segment_distance(
            cursor,
            project(frame.along(axis, style.centre))?,
            project(frame.along(axis, style.size))?,
        ))
    };
    if let Some(only) = handles.only {
        return nearest_axis(style.pick_radius, |axis| {
            (axis == only).then(|| arrow(axis)).flatten()
        });
    }
    if handles.mode == GizmoMode::Rotate {
        let ring = |arc: Arc, pixels: f32| {
            let ring = frame
                .ring(arc, pixels)
                .map(&project)
                .collect::<Option<Vec<_>>>()?;
            ring.windows(2)
                .map(|ends| {
                    segment_distance(cursor, ends[0], ends[1])
                })
                .min_by(f32::total_cmp)
        };
        return nearest_axis(style.pick_radius, |axis| {
            ring(frame.arc(axis, style), style.size)
        })
        .or_else(|| {
            ring(Arc::full(frame.look), style.outer_ring)
                .filter(|distance| *distance <= style.pick_radius)
                .map(|distance| (Handle::View, distance))
        })
        .or_else(|| {
            (from_centre <= style.size)
                .then_some((Handle::Ball, from_centre))
        });
    }
    if from_centre <= style.centre.max(style.pick_radius) {
        return Some((Handle::Centre, from_centre));
    }
    let on_plane = |axis: &usize| {
        frame.plane_fade(*axis, style) > 0.0
            && frame
                .plane(*axis, style)
                .into_iter()
                .map(&project)
                .collect::<Option<Vec<_>>>()
                .is_some_and(|corners| is_inside(cursor, &corners))
    };
    nearest_axis(style.pick_radius, arrow)
        .or_else(|| {
            (0..3)
                .find(on_plane)
                .map(|axis| (Handle::Plane(axis), style.pick_radius))
        })
        .or_else(|| {
            // A scale is even from anywhere else inside its ring.
            (handles.mode == GizmoMode::Scale
                && from_centre
                    <= style.outer_ring + style.pick_radius)
                .then_some((Handle::Centre, from_centre))
        })
}

/// Finds the handle under the preview's pointer, and draws a gizmo on
/// every target.
#[expect(
    clippy::too_many_arguments,
    reason = "it picks and draws every gizmo"
)]
fn handles(
    mut gizmos: Gizmos<HandleGizmos>,
    editor: Res<Editor<Blueprint>>,
    mode: Res<GizmoMode>,
    dragging: Res<Dragging>,
    buttons: Res<ButtonInput<MouseButton>>,
    preview: Option<Res<Preview>>,
    If(theme): If<Res<Theme<HamletTheme>>>,
    background: Res<Background>,
    frames: Query<
        (&ComputedNode, &UiGlobalTransform),
        With<PreviewFrame>,
    >,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    filled: Single<(&Mesh3d, &mut Visibility), With<HandleFills>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut hot: ResMut<Hot>,
    mut drawn: Local<Fills>,
) {
    let palette = &theme.0.palette;
    let style = Style::of(palette).against(palette, *background);
    let mut fills = Fills::default();
    let view = preview
        .as_ref()
        .and_then(|preview| cameras.get(preview.camera).ok());
    let corner =
        frames.iter().next().map(|(node, at)| corner(node, at));
    let cursor = windows
        .iter()
        .next()
        .and_then(Window::cursor_position)
        .zip(corner)
        .map(|(cursor, corner)| cursor - corner);
    if let Some((camera, eye)) = view {
        let project =
            |point: Vec3| camera.world_to_viewport(eye, point).ok();
        let drag = dragging.0.as_ref();
        let mut nearest: Option<(usize, Handle, f32)> = None;
        let targets = targets(&editor);
        for (index, target) in targets.iter().enumerate() {
            let (handles, turn) = handles_of(target, *mode);
            let Some(mut frame) =
                Frame::of(target.at, turn, camera, eye)
            else {
                continue;
            };
            let held = drag.filter(|drag| drag.index == index);
            if let Some(drag) = held {
                // The axes it took hold of, which a turn would carry
                // off.
                frame.axes = drag.grab.frame.axes;
            } else if drag.is_none()
                && !buttons.pressed(MouseButton::Left)
                && let Some(cursor) = cursor
                && let Some((handle, distance)) =
                    pick(handles, &frame, cursor, &style, project)
                && nearest.is_none_or(|(.., best)| distance < best)
            {
                nearest = Some((index, handle, distance));
            }
            let hot_here = match held {
                Some(drag) => Some(drag.grab.handle),
                None => hot
                    .0
                    .filter(|(at, _)| *at == index)
                    .map(|(_, handle)| handle),
            };
            let shown = Shown {
                frame,
                hot: hot_here,
                dragging: held.is_some(),
                sweep: held
                    .filter(|_| handles.mode == GizmoMode::Rotate)
                    .and_then(|drag| {
                        drag.grab.sweep(drag.angle, &style)
                    }),
            };
            paint(&mut gizmos, &mut fills, handles, &shown, &style);
        }
        if drag.is_none() && !buttons.pressed(MouseButton::Left) {
            let found =
                nearest.map(|(index, handle, _)| (index, handle));
            if hot.0 != found {
                hot.0 = found;
            }
        }
    } else if hot.0.is_some() {
        hot.0 = None;
    }

    let (mesh, mut visibility) = filled.into_inner();
    let empty = fills.positions.is_empty();
    visibility.set_if_neq(if empty {
        Visibility::Hidden
    } else {
        Visibility::Visible
    });
    if !empty
        && *drawn != fills
        && let Some(mut mesh) = meshes.get_mut(&mesh.0)
    {
        *mesh = fills.mesh();
        *drawn = fills;
    }
}
