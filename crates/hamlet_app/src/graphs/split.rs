//! The line between the canvas and the preview, dragged to size them,
//! and whether the properties float open over the canvas.

use bevy::app::{App, Update};
use bevy::ecs::component::Component;
use bevy::ecs::observer::On;
use bevy::ecs::query::{Changed, With};
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{If, Query, Res, ResMut};
use bevy::picking::events::{
    PointerDrag, PointerDragEnd, PointerDragStart,
};
use bevy::picking::hover::Hovered;
use bevy::ui::{BackgroundColor, px};
use bevy::window::SystemCursorIcon;
use bevy_fynix::views::{BehaviorExt, frame};
use bevy_fynix::{EntityCursor, OverrideCursor, Theme};

use crate::theme::HamletTheme;
use crate::{View, with_cx};

/// How thick the divider is.
const THICK: f32 = 6.0;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Layout>()
        .add_observer(grab)
        .add_observer(drag)
        .add_observer(drop)
        .add_systems(Update, paint);
}

/// How the Graph panel is laid out.
#[derive(Resource)]
pub(super) struct Layout {
    /// The width of the preview, in logical pixels.
    pub preview: f32,
    /// Whether the properties are open over the canvas, or folded to
    /// their header.
    pub properties: bool,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            preview: 360.0,
            properties: true,
        }
    }
}

/// Marks the line between the canvas and the preview.
#[derive(Component)]
struct Divider;

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

fn grab(
    start: On<PointerDragStart>,
    dividers: Query<(), With<Divider>>,
    mut cursor: ResMut<OverrideCursor>,
) {
    if dividers.contains(start.entity) {
        cursor.0 = Some(SystemCursorIcon::ColResize);
    }
}

fn drag(
    drag: On<PointerDrag>,
    dividers: Query<(), With<Divider>>,
    mut layout: ResMut<Layout>,
) {
    if dividers.contains(drag.entity) {
        layout.preview =
            (layout.preview - drag.delta.x).clamp(200.0, 1200.0);
    }
}

fn drop(
    end: On<PointerDragEnd>,
    dividers: Query<(), With<Divider>>,
    mut cursor: ResMut<OverrideCursor>,
) {
    if dividers.contains(end.entity) {
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
