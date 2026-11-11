//! The blueprint preview's gizmo on the selected object in the
//! PREFAB view: its handles drawn and picked as there, G, R and S
//! picking its mode, and a drag written to the object's part. A move
//! and a turn go along the world's axes and a scale along the
//! object's own, as for a transform node; Ctrl snaps, and Escape puts
//! the part back. The gizmo stands at the middle of the object's
//! geometry, which a turn or a scale keeps in place.

use bevy::asset::Assets;
use bevy::camera::Camera;
use bevy::camera::primitives::Aabb;
use bevy::camera::visibility::{RenderLayers, Visibility};
use bevy::ecs::component::Component;
use bevy::ecs::event::EntityEvent;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{
    Commands, If, Local, Query, Res, ResMut, Single,
};
use bevy::ecs::world::World;
use bevy::gizmos::config::{GizmoConfigGroup, GizmoConfigStore};
use bevy::gizmos::gizmos::Gizmos;
use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
use bevy::input::mouse::MouseButton;
use bevy::math::{Quat, Vec2, Vec3};
use bevy::mesh::{Mesh, Mesh3d};
use bevy::picking::events::{
    PointerDrag, PointerDragEnd, PointerDragStart,
};
use bevy::picking::pointer::PointerButton;
use bevy::reflect::Reflect;
use bevy::transform::components::{GlobalTransform, Transform};
use bevy::ui::{ComputedNode, UiGlobalTransform};
use bevy::window::{PrimaryWindow, Window};
use bevy_fynix::Theme;
use hamlet_graph::prefab::Part;
use hamlet_graph_editor::canvas::corner;
use hamlet_ui::theme::HamletTheme;

use super::super::gizmo::fills::{FillMaterial, Fills, spawn_shape};
use super::super::gizmo::{
    Background, Frame, GizmoMode, Grab, Handle, Handles, Pointing,
    Shape, Shown as Painted, Style, paint, pick,
};
use super::{LAYER, Open, Shown, Stage, edit_part, placed};

/// The gizmo's lines, seen only by the PREFAB view's camera.
#[derive(Default, Reflect, GizmoConfigGroup)]
pub(super) struct PrefabHandleGizmos;

pub(super) fn style(mut store: ResMut<GizmoConfigStore>) {
    let (config, _) = store.config_mut::<PrefabHandleGizmos>();
    config.render_layers = RenderLayers::layer(LAYER);
    config.line.width = 3.0;
    // In front of the object the handles sit inside.
    config.depth_bias = -1.0;
}

/// Marks the mesh of the gizmo's filled shapes in the PREFAB view.
#[derive(Component)]
pub(super) struct PrefabFills;

pub(super) fn spawn(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<FillMaterial>>,
) {
    spawn_shape(
        &mut commands,
        &mut meshes,
        &mut materials,
        PrefabFills,
        RenderLayers::layer(LAYER),
    );
}

/// The handle under the pointer on the view.
#[derive(Resource, Default)]
pub(super) struct Hot(pub(super) Option<Handle>);

/// A drag of a handle under way: how it began, the part as it was,
/// and the object as the world had it then.
struct Drag {
    grab: Grab,
    mode: GizmoMode,
    part: Part,
    /// The object's parent, which the part's offset is in.
    parent: GlobalTransform,
    /// The object's turn in the world.
    rotation: Quat,
    /// The angle a turn has turned it by, for the sweep drawn.
    angle: f32,
    /// The object's place in its parent as the file gives it.
    file: Transform,
    /// The middle of its geometry, in its own space, which a turn
    /// and a scale keep in place.
    pivot: Vec3,
}

/// The gizmo drag under way on the view, of which there is one at
/// most.
#[derive(Resource, Default)]
pub(super) struct Dragging(Option<Drag>);

impl Dragging {
    /// Whether a handle is dragged, so the drag does not also turn
    /// the view.
    pub(super) fn is_under_way(&self) -> bool {
        self.0.is_some()
    }
}

/// The selected object's place in the world and its place in its
/// parent.
fn selected(
    open: &Open,
    objects: &Query<(&GlobalTransform, &Transform)>,
) -> Option<(GlobalTransform, Transform)> {
    let path = open.selected.as_ref()?;
    let loaded = open.model.as_ref()?;
    let (entity, _) =
        loaded.entities.get(loaded.model.find(path)?)?;
    let (at, local) = objects.get(*entity).ok()?;
    Some((*at, *local))
}

/// The middle of the box around the selected object's meshes in the
/// world: its own, or, for an object with none, those of the objects
/// inside it. None before any is laid out.
fn centre(
    open: &Open,
    boxes: &Query<(&Aabb, &GlobalTransform)>,
) -> Option<Vec3> {
    let path = open.selected.as_ref()?;
    let loaded = open.model.as_ref()?;
    let own = loaded.model.find(path)?;
    let inside = format!("{path}/");
    let mut pieces: Vec<_> = loaded.entities.get(own)?.1.clone();
    if pieces.is_empty() {
        for (object, (_, drawn)) in
            loaded.model.objects.iter().zip(&loaded.entities)
        {
            if object.path.starts_with(&inside) {
                pieces.extend(drawn.iter().copied());
            }
        }
    }
    let mut low = Vec3::splat(f32::INFINITY);
    let mut high = Vec3::splat(f32::NEG_INFINITY);
    for piece in pieces {
        let Ok((aabb, at)) = boxes.get(piece) else {
            continue;
        };
        let (middle, half) =
            (Vec3::from(aabb.center), Vec3::from(aabb.half_extents));
        for corner in 0..8 {
            let sign = Vec3::new(
                if corner & 1 == 0 { -1.0 } else { 1.0 },
                if corner & 2 == 0 { -1.0 } else { 1.0 },
                if corner & 4 == 0 { -1.0 } else { 1.0 },
            );
            let point = at.transform_point(middle + half * sign);
            low = low.min(point);
            high = high.max(point);
        }
    }
    low.cmple(high).all().then(|| (low + high) * 0.5)
}

/// The gizmo's handles in `mode`, and the turn of its axes: the
/// object's own for a scale, the world's otherwise.
fn handles_in(
    mode: GizmoMode,
    at: &GlobalTransform,
) -> (Handles, Quat) {
    let turn = if mode == GizmoMode::Scale {
        at.rotation()
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

/// The pointer at `at` in the window, over the view, through its
/// camera.
fn pointing(
    at: Vec2,
    frames: &Query<(&ComputedNode, &UiGlobalTransform), With<Shown>>,
    camera: &Camera,
    eye: &GlobalTransform,
) -> Option<Pointing> {
    let (node, transform) = frames.iter().next()?;
    let cursor = at - corner(node, transform);
    let ray = camera.viewport_to_world(eye, cursor).ok()?;
    Some(Pointing { cursor, ray })
}

/// Finds the handle under the pointer and draws the gizmo on the
/// selected object.
#[expect(
    clippy::too_many_arguments,
    reason = "it picks and draws the gizmo"
)]
pub(super) fn handles(
    mut gizmos: Gizmos<PrefabHandleGizmos>,
    open: Res<Open>,
    mode: Res<GizmoMode>,
    dragging: Res<Dragging>,
    buttons: Res<ButtonInput<MouseButton>>,
    stage: Option<Res<Stage>>,
    If(theme): If<Res<Theme<HamletTheme>>>,
    background: Res<Background>,
    frames: Query<(&ComputedNode, &UiGlobalTransform), With<Shown>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    // Together, as a system takes at most 16 parameters.
    (objects, boxes): (
        Query<(&GlobalTransform, &Transform)>,
        Query<(&Aabb, &GlobalTransform)>,
    ),
    filled: Single<(&Mesh3d, &mut Visibility), With<PrefabFills>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut hot: ResMut<Hot>,
    mut drawn: Local<Fills>,
) {
    let palette = &theme.0.palette;
    let style = Style::of(palette).against(palette, *background);
    let mut fills = Fills::default();
    let mut found = None;
    let view = stage
        .as_ref()
        .and_then(|stage| cameras.get(stage.camera).ok());
    if let (Some((camera, eye)), Some((at, _))) =
        (view, selected(&open, &objects))
    {
        let drag = dragging.0.as_ref();
        let wanted = drag.map_or(*mode, |drag| drag.mode);
        let (handles, turn) = handles_in(wanted, &at);
        let middle =
            centre(&open, &boxes).unwrap_or(at.translation());
        if let Some(mut frame) = Frame::of(middle, turn, camera, eye)
        {
            let project = |point: Vec3| {
                camera.world_to_viewport(eye, point).ok()
            };
            let cursor = windows
                .iter()
                .next()
                .and_then(Window::cursor_position)
                .zip(frames.iter().next())
                .map(|(cursor, (node, at))| {
                    cursor - corner(node, at)
                });
            if let Some(drag) = drag {
                // The axes it took hold of, which a turn would carry
                // off.
                frame.axes = drag.grab.frame.axes;
            } else if !buttons.pressed(MouseButton::Left)
                && let Some(cursor) = cursor
            {
                found =
                    pick(handles, &frame, cursor, &style, project)
                        .map(|(handle, _)| handle);
            }
            let shown = Painted {
                frame,
                hot: drag.map(|drag| drag.grab.handle).or(hot.0),
                dragging: drag.is_some(),
                sweep: drag
                    .filter(|drag| drag.mode == GizmoMode::Rotate)
                    .and_then(|drag| {
                        drag.grab.sweep(drag.angle, &style)
                    }),
            };
            paint(&mut gizmos, &mut fills, handles, &shown, &style);
        }
    }
    if dragging.0.is_none()
        && !buttons.pressed(MouseButton::Left)
        && hot.0 != found
    {
        hot.0 = found;
    }
    let (mesh, visibility) = filled.into_inner();
    fills.show(&mut drawn, mesh, visibility, &mut meshes);
}

/// Takes hold of the handle under the pointer when a drag begins on
/// it; the drag then moves the handle, not the view.
#[expect(
    clippy::too_many_arguments,
    reason = "a drag reads all it takes hold of"
)]
pub(super) fn grab(
    start: On<PointerDragStart>,
    mut open: ResMut<Open>,
    mode: Res<GizmoMode>,
    hot: Res<Hot>,
    stage: Option<Res<Stage>>,
    If(theme): If<Res<Theme<HamletTheme>>>,
    frames: Query<(&ComputedNode, &UiGlobalTransform), With<Shown>>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    objects: Query<(&GlobalTransform, &Transform)>,
    boxes: Query<(&Aabb, &GlobalTransform)>,
    mut dragging: ResMut<Dragging>,
) {
    if start.button != PointerButton::Primary
        || frames.get(start.event_target()).is_err()
    {
        return;
    }
    let (Some(handle), Some(stage), Some(path)) =
        (hot.0, stage, open.selected.clone())
    else {
        return;
    };
    let Ok((camera, eye)) = cameras.get(stage.camera) else {
        return;
    };
    let Some((at, local)) = selected(&open, &objects) else {
        return;
    };
    let (handles, turn) = handles_in(*mode, &at);
    let middle = centre(&open, &boxes).unwrap_or(at.translation());
    let Some(frame) = Frame::of(middle, turn, camera, eye) else {
        return;
    };
    let Some(file) = open.model.as_ref().and_then(|loaded| {
        let at = loaded.model.find(&path)?;
        Some(loaded.model.objects.get(at)?.local)
    }) else {
        return;
    };
    let pivot = at.affine().inverse().transform_point3(middle);
    let (Some(now), Ok(centre)) = (
        pointing(start.pointer.position, &frames, camera, eye),
        camera.world_to_viewport(eye, frame.origin),
    ) else {
        return;
    };
    let parent = GlobalTransform::from(
        at.affine() * local.compute_affine().inverse(),
    );
    // The drag is one step to undo.
    open.history.holding = true;
    open.history.at = None;
    dragging.0 = Some(Drag {
        grab: Grab {
            handle,
            frame,
            pointing: now,
            centre,
            reach: Style::of(&theme.0.palette).size,
        },
        mode: handles.mode,
        part: open.prefab.part(&path),
        parent,
        rotation: at.rotation(),
        angle: 0.0,
        file,
        pivot,
    });
}

/// Writes the drag under way to the part as the pointer moves. Ctrl
/// snaps it.
pub(super) fn steer(
    drag: On<PointerDrag>,
    mut commands: Commands,
    stage: Option<Res<Stage>>,
    keys: Res<ButtonInput<KeyCode>>,
    If(theme): If<Res<Theme<HamletTheme>>>,
    frames: Query<(&ComputedNode, &UiGlobalTransform), With<Shown>>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    mut dragging: ResMut<Dragging>,
) {
    let (Some(held), Some(stage)) = (dragging.0.as_mut(), stage)
    else {
        return;
    };
    let Ok((camera, eye)) = cameras.get(stage.camera) else {
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
    let mut part = held.part.clone();
    match held.mode {
        GizmoMode::Translate => {
            let Some(delta) = held.grab.translation(
                &now,
                snaps.then_some(style.translate_snap),
            ) else {
                return;
            };
            let moved = held
                .parent
                .affine()
                .inverse()
                .transform_vector3(delta);
            part.offset = round(held.part.offset + moved);
        }
        GizmoMode::Rotate => {
            let Some((axis, angle)) = held
                .grab
                .turn(&now, snaps.then_some(style.rotate_snap))
            else {
                return;
            };
            held.angle = angle;
            // The turn above the part's: the parent's and the file's.
            let above = held.rotation * held.part.turn.inverse();
            let turned =
                Quat::from_axis_angle(axis, angle) * held.rotation;
            part.turn = (above.inverse() * turned).normalize();
        }
        GizmoMode::Scale => {
            let Some(factor) = held
                .grab
                .scale(&now, snaps.then_some(style.scale_snap))
            else {
                return;
            };
            part.scale = round(held.part.scale * factor);
        }
    }
    if held.mode != GizmoMode::Translate {
        // Turned or scaled about the object's origin, its middle
        // moved: move it back by as much, in its parent's space.
        let was =
            placed(held.file, &held.part).transform_point(held.pivot);
        let now =
            placed(held.file, &part).transform_point(held.pivot);
        part.offset = round(held.part.offset + was - now);
    }
    commands.queue(move |world: &mut World| {
        let unchanged = {
            let open = world.resource::<Open>();
            open.selected
                .as_ref()
                .is_none_or(|path| open.prefab.part(path) == part)
        };
        if !unchanged {
            edit_part(world, move |was| *was = part);
        }
    });
}

/// Lets go of the handle, keeping what it set.
pub(super) fn release(
    end: On<PointerDragEnd>,
    mut open: ResMut<Open>,
    mut dragging: ResMut<Dragging>,
) {
    if end.button == PointerButton::Primary && dragging.0.is_some() {
        dragging.0 = None;
        open.history.holding = false;
    }
}

/// Lets Escape take back the drag under way, putting the part back.
pub(super) fn cancel(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    mut dragging: ResMut<Dragging>,
) {
    if dragging.0.is_none() || !keys.just_pressed(KeyCode::Escape) {
        return;
    }
    if let Some(drag) = dragging.0.take() {
        let part = drag.part;
        commands.queue(move |world: &mut World| {
            edit_part(world, move |was| *was = part);
            world.resource_mut::<Open>().history.holding = false;
        });
    }
}

/// A point to the centimetre, so dragged values stay readable.
fn round(point: Vec3) -> Vec3 {
    (point * 100.0).round() / 100.0
}
