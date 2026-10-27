//! How the Graph panel is laid out: the line between the canvas and
//! the preview, dragged to size them, and the properties floating
//! over the canvas. The properties are dragged by their header; while
//! they are, an outline shows the corner they will settle in when let
//! go, and they glide there.

use bevy::app::{App, Update};
use bevy::ecs::change_detection::DetectChangesMut;
use bevy::ecs::component::Component;
use bevy::ecs::observer::On;
use bevy::ecs::query::{Changed, With};
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{If, Query, Res, ResMut};
use bevy::math::Vec2;
use bevy::picking::events::{
    PointerDrag, PointerDragEnd, PointerDragStart,
};
use bevy::picking::hover::Hovered;
use bevy::time::Time;
use bevy::ui::{BackgroundColor, ComputedNode, UiRect, Val, px};
use bevy::window::SystemCursorIcon;
use bevy_fynix::views::{BehaviorExt, frame};
use bevy_fynix::{EntityCursor, OverrideCursor, Theme};

use super::canvas::Canvas;
use crate::theme::HamletTheme;
use crate::{View, with_cx};

/// How thick the divider is.
const THICK: f32 = 6.0;

/// How far the properties sit from the canvas's edges.
const MARGIN: f32 = 8.0;

/// How fast the properties close on where they are going, per second.
const GLIDE: f32 = 14.0;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Layout>()
        .add_observer(grab)
        .add_observer(drag)
        .add_observer(drop)
        .add_systems(Update, (glide, paint));
}

/// A corner of the canvas.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Corner {
    /// The corner of an area half `half` in size nearest `middle`.
    fn nearest(middle: Vec2, half: Vec2) -> Self {
        match (middle.x < half.x, middle.y < half.y) {
            (true, true) => Corner::TopLeft,
            (false, true) => Corner::TopRight,
            (true, false) => Corner::BottomLeft,
            (false, false) => Corner::BottomRight,
        }
    }

    /// The top left of a box of `size` in this corner of `area`.
    fn place(self, area: Vec2, size: Vec2) -> Vec2 {
        let far = (area - size - Vec2::splat(MARGIN))
            .max(Vec2::splat(MARGIN));
        match self {
            Corner::TopLeft => Vec2::splat(MARGIN),
            Corner::TopRight => Vec2::new(far.x, MARGIN),
            Corner::BottomLeft => Vec2::new(MARGIN, far.y),
            Corner::BottomRight => far,
        }
    }
}

/// How the Graph panel is laid out. Places are in logical pixels from
/// the canvas's top left corner.
#[derive(Resource)]
pub(super) struct Layout {
    /// The width of the preview.
    pub preview: f32,
    /// Whether the properties are open, or folded to their header.
    pub properties: bool,
    /// The corner the properties settle in.
    pub corner: Corner,
    /// Where the properties are drawn.
    pub shown: Vec2,
    /// Where the properties would settle if let go now, while
    /// dragged.
    pub ghost: Option<Vec2>,
    /// The properties' size as drawn.
    pub size: Vec2,
    /// Where the properties' top left is while they are dragged.
    held: Option<Vec2>,
    /// Whether the properties have been placed once, after which
    /// they glide.
    placed: bool,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            preview: 360.0,
            properties: true,
            corner: Corner::TopRight,
            shown: Vec2::splat(MARGIN),
            ghost: None,
            size: Vec2::ZERO,
            held: None,
            placed: false,
        }
    }
}

/// The inset that puts a box's top left at `at`.
pub(super) fn inset_at(at: Vec2) -> UiRect {
    UiRect::new(px(at.x), Val::Auto, px(at.y), Val::Auto)
}

/// Marks the line between the canvas and the preview.
#[derive(Component)]
struct Divider;

/// Marks the properties floating over the canvas.
#[derive(Component)]
pub(super) struct Floating;

/// Marks the header the properties are dragged by.
#[derive(Component)]
pub(super) struct Handle;

pub(super) fn divider() -> impl View {
    with_cx(|cx| {
        frame()
            .fill(cx.theme().palette.seam)
            .radius(2.0)
            .shrink(0.0)
            .width(px(THICK))
            .tagged(Divider)
            .tagged(Hovered::default())
            .tagged(EntityCursor(SystemCursorIcon::ColResize))
    })
}

fn logical(node: &ComputedNode) -> Vec2 {
    node.size() * node.inverse_scale_factor()
}

/// Moves the properties toward where they belong: under the pointer
/// while dragged, else their corner, easing there from where they
/// are.
fn glide(
    time: Res<Time>,
    floating: Query<&ComputedNode, With<Floating>>,
    canvas: Query<&ComputedNode, With<Canvas>>,
    mut layout: ResMut<Layout>,
) {
    let (Some(panel), Some(area)) =
        (floating.iter().next(), canvas.iter().next())
    else {
        return;
    };
    let (size, area) = (logical(panel), logical(area));
    let now = layout.bypass_change_detection();
    let ghost = now.held.map(|at| {
        Corner::nearest(at + size * 0.5, area * 0.5).place(area, size)
    });
    let target =
        now.held.unwrap_or_else(|| now.corner.place(area, size));
    let shown = if now.placed {
        let step = 1.0 - (-GLIDE * time.delta_secs()).exp();
        let next = now.shown + (target - now.shown) * step;
        if next.distance(target) < 0.25 {
            target
        } else {
            next
        }
    } else {
        target
    };
    if now.shown != shown
        || now.ghost != ghost
        || now.size != size
        || !now.placed
    {
        let layout = layout.as_mut();
        layout.shown = shown;
        layout.ghost = ghost;
        layout.size = size;
        layout.placed = true;
    }
}

fn grab(
    start: On<PointerDragStart>,
    dividers: Query<(), With<Divider>>,
    handles: Query<(), With<Handle>>,
    mut layout: ResMut<Layout>,
    mut cursor: ResMut<OverrideCursor>,
) {
    if dividers.contains(start.entity) {
        cursor.0 = Some(SystemCursorIcon::ColResize);
    } else if handles.contains(start.entity) {
        layout.held = Some(layout.shown);
        cursor.0 = Some(SystemCursorIcon::Grabbing);
    }
}

fn drag(
    drag: On<PointerDrag>,
    dividers: Query<(), With<Divider>>,
    handles: Query<(), With<Handle>>,
    mut layout: ResMut<Layout>,
) {
    if dividers.contains(drag.entity) {
        layout.preview =
            (layout.preview - drag.delta.x).clamp(200.0, 1200.0);
    } else if handles.contains(drag.entity)
        && let Some(at) = &mut layout.held
    {
        *at += drag.delta;
    }
}

/// Settles dragged properties in the corner of the canvas nearest
/// their middle.
fn drop(
    end: On<PointerDragEnd>,
    dividers: Query<(), With<Divider>>,
    handles: Query<(), With<Handle>>,
    canvas: Query<&ComputedNode, With<Canvas>>,
    mut layout: ResMut<Layout>,
    mut cursor: ResMut<OverrideCursor>,
) {
    if handles.contains(end.entity)
        && let Some(at) = layout.held.take()
        && let Some(area) = canvas.iter().next()
    {
        let middle = at + layout.size * 0.5;
        layout.corner = Corner::nearest(middle, logical(area) * 0.5);
        layout.ghost = None;
    }
    if dividers.contains(end.entity) || handles.contains(end.entity) {
        cursor.0 = None;
    }
}

/// Lights the divider under the pointer.
fn paint(
    If(theme): If<Res<Theme<HamletTheme>>>,
    mut dividers: Query<
        (&Hovered, &mut BackgroundColor),
        (Changed<Hovered>, With<Divider>),
    >,
) {
    let palette = &theme.0.palette;
    for (hovered, mut fill) in &mut dividers {
        fill.0 = if hovered.get() {
            palette.brass_5
        } else {
            palette.seam
        };
    }
}
