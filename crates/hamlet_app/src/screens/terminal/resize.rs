//! The line between a Terminal panel's screen and its list: dragged,
//! it sizes the list, between [`NARROWEST`] and what leaves the
//! screen [`SCREEN`].

use bevy::app::App;
use bevy::ecs::component::Component;
use bevy::ecs::observer::On;
use bevy::ecs::system::{Query, ResMut};
use bevy::picking::events::{
    PointerDrag, PointerDragEnd, PointerDragStart,
};
use bevy::ui::{ComputedNode, UiGlobalTransform};
use bevy::window::SystemCursorIcon;
use bevy_fynix::tokens::MotionTokens;
use bevy_fynix::views::{self, Axis, BehaviorExt};
use bevy_fynix::{Dragging, OverrideCursor, ScopedExt, style};
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::{Cx, View};

use super::{Edge, PanelId, Panels};

/// The least width of a list, in logical pixels.
pub(super) const NARROWEST: f32 = 180.0;

/// The least width a list leaves the screen beside it.
const SCREEN: f32 = 240.0;

pub(super) fn plugin(app: &mut App) {
    app.add_observer(grab).add_observer(drag).add_observer(drop);
}

/// The line between the screen and the list of `panel`.
#[derive(Component)]
struct Line(PanelId);

/// The line before the list of `panel`, which sizes it when dragged.
pub(super) fn line(panel: &PanelId) -> impl View + use<> {
    views::divider(Axis::Vertical)
        .rules(lit)
        .tagged(Line(panel.clone()))
}

/// The line is a hairline, in the accent while held or hovered.
fn lit(cx: &mut Cx) {
    style::<HamletTheme>()
        .frame(|f, t| f.fill(t.palette.border))
        .hovered(|s| s.fill(|t: &HamletTheme| t.palette.gold_4))
        .when::<Dragging>(|s| {
            s.fill(|t: &HamletTheme| t.palette.gold_4)
        })
        .transition(HamletTheme::interact)
        .apply(cx);
}

fn grab(
    start: On<PointerDragStart>,
    lines: Query<&Line>,
    mut cursor: ResMut<OverrideCursor>,
) {
    if lines.contains(start.entity) {
        cursor.0 = Some(SystemCursorIcon::EwResize);
    }
}

/// Sizes the list so that the line is where the pointer is. The list
/// runs to the panel's right edge, which its [`Edge`] covers.
fn drag(
    drag: On<PointerDrag>,
    lines: Query<&Line>,
    edges: Query<(&Edge, &ComputedNode, &UiGlobalTransform)>,
    mut panels: ResMut<Panels>,
) {
    let Ok(Line(panel)) = lines.get(drag.entity) else {
        return;
    };
    let Some((_, node, place)) =
        edges.iter().find(|(edge, ..)| edge.0 == *panel)
    else {
        return;
    };
    let scale = node.inverse_scale_factor();
    let wide = node.size().x * scale;
    let right = (place.translation.x + node.size().x * 0.5) * scale;
    let width = fit(right - drag.pointer.position.x, wide);
    panels.edit(panel).list_width = Some(width);
}

/// The width a list is drawn at when the line is `want` from the
/// right edge of a panel `wide`: at least [`NARROWEST`], and no wider
/// than leaves the screen [`SCREEN`].
fn fit(want: f32, wide: f32) -> f32 {
    want.clamp(NARROWEST, (wide - SCREEN).max(NARROWEST))
}

fn drop(
    end: On<PointerDragEnd>,
    lines: Query<&Line>,
    mut cursor: ResMut<OverrideCursor>,
) {
    if lines.contains(end.entity) {
        cursor.0 = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_list_keeps_its_least_width_and_leaves_the_screen_room() {
        assert_eq!(fit(300.0, 1000.0), 300.0);
        assert_eq!(fit(20.0, 1000.0), NARROWEST);
        assert_eq!(fit(900.0, 1000.0), 1000.0 - SCREEN);
        // A panel too narrow for both keeps the list's least width.
        assert_eq!(fit(300.0, 300.0), NARROWEST);
    }
}
