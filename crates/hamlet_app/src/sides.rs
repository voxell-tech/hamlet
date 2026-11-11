//! The columns either side of a panel's middle, each sized by
//! dragging the line between it and the middle. A panel names its
//! columns with a type of its own, `P`, so each keeps its widths.

use std::marker::PhantomData;

use bevy::app::App;
use bevy::ecs::component::Component;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Query, ResMut};
use bevy::picking::events::{
    PointerDrag, PointerDragEnd, PointerDragStart,
};
use bevy::ui::{Val, px};
use bevy::window::SystemCursorIcon;
use bevy_fynix::views::{self, Axis, BehaviorExt};
use bevy_fynix::{
    Dragging, OverrideCursor, ScopedExt, Signal, ViewExt, resource,
    style,
};
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::{AnyView, Cx};

/// The narrowest and widest a column is dragged to, in pixels.
const NARROWEST: f32 = 160.0;
const WIDEST: f32 = 640.0;

/// Which column of a panel.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Side {
    Left,
    Right,
}

/// How wide the columns of the panel `P` are, as dragged.
#[derive(Resource)]
pub(crate) struct Sides<P: Send + Sync + 'static> {
    left: f32,
    right: f32,
    /// The width of the column being dragged when the drag began.
    from: f32,
    panel: PhantomData<P>,
}

impl<P: Send + Sync + 'static> Sides<P> {
    fn width(&mut self, side: Side) -> &mut f32 {
        match side {
            Side::Left => &mut self.left,
            Side::Right => &mut self.right,
        }
    }
}

/// Gives the panel `P` columns `left` and `right` pixels wide to
/// start with.
pub(crate) fn plugin<P: Send + Sync + 'static>(
    app: &mut App,
    left: f32,
    right: f32,
) {
    app.insert_resource(Sides::<P> {
        left,
        right,
        from: left,
        panel: PhantomData,
    })
    .add_observer(grab::<P>)
    .add_observer(drag::<P>)
    .add_observer(drop::<P>);
}

/// How wide the column `side` of the panel `P` is now.
pub(crate) fn width<P: Send + Sync + 'static>(
    side: Side,
) -> Signal<Val> {
    resource::<Sides<P>, _>(move |sides| {
        px(match side {
            Side::Left => sides.left,
            Side::Right => sides.right,
        })
    })
}

/// Marks the line beside the column `side` of the panel `P`.
#[derive(Component)]
struct Line<P: Send + Sync + 'static>(Side, PhantomData<P>);

/// The line between the column `side` of the panel `P` and the
/// middle, dragged to size the column.
pub(crate) fn line<P: Send + Sync + 'static>(side: Side) -> AnyView {
    views::divider(Axis::Vertical)
        .rules(line_rules)
        .tagged(Line::<P>(side, PhantomData))
        .boxed()
}

/// A hairline, in the accent while held or hovered.
fn line_rules(cx: &mut Cx) {
    style::<HamletTheme>()
        .frame(|f, t| f.fill(t.palette.gold_6))
        .hovered(|s| s.fill(|t: &HamletTheme| t.palette.gold_4))
        .when::<Dragging>(|s| {
            s.fill(|t: &HamletTheme| t.palette.gold_4)
        })
        .apply(cx);
}

fn grab<P: Send + Sync + 'static>(
    start: On<PointerDragStart>,
    lines: Query<&Line<P>>,
    mut sides: ResMut<Sides<P>>,
    mut cursor: ResMut<OverrideCursor>,
) {
    if let Ok(Line(side, _)) = lines.get(start.entity) {
        sides.from = *sides.width(*side);
        cursor.0 = Some(SystemCursorIcon::EwResize);
    }
}

/// Sizes the column so its edge follows the pointer: the left one
/// grows to the right, the right one to the left.
fn drag<P: Send + Sync + 'static>(
    drag: On<PointerDrag>,
    lines: Query<&Line<P>>,
    mut sides: ResMut<Sides<P>>,
) {
    let Ok(Line(side, _)) = lines.get(drag.entity) else {
        return;
    };
    let moved = match side {
        Side::Left => drag.distance.x,
        Side::Right => -drag.distance.x,
    };
    let width = (sides.from + moved).clamp(NARROWEST, WIDEST);
    let at = sides.width(*side);
    if *at != width {
        *at = width;
    }
}

fn drop<P: Send + Sync + 'static>(
    end: On<PointerDragEnd>,
    lines: Query<(), With<Line<P>>>,
    mut cursor: ResMut<OverrideCursor>,
) {
    if lines.contains(end.entity) {
        cursor.0 = None;
    }
}
