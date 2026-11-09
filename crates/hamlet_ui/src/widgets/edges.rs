//! The ends of a scroll area fade into what it sits on while there is
//! more to scroll that way: a gradient laid over each end, from clear
//! to the first opaque fill behind the area. The rows under it stay
//! whole, letters and all, and only darken or lighten together.

use bevy::app::{App, PostUpdate};
use bevy::color::{Alpha, Color};
use bevy::ecs::change_detection::DetectChangesMut;
use bevy::ecs::component::Component;
use bevy::ecs::hierarchy::{ChildOf, Children};
use bevy::ecs::query::With;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::Query;
use bevy::picking::Pickable;
use bevy::ui::{
    BackgroundColor, BackgroundGradient, ColorStop, ComputedNode,
    LinearGradient, PositionType, UiRect, UiSystems, Val, percent,
    px,
};
use bevy_fynix::scroll::ScrollGoal;
use bevy_fynix::views::{BehaviorExt, FrameProps, column, frame};

use crate::View;
use crate::styles::Fills;

/// How far in from an end of a scroll area its fade reaches, in
/// logical pixels.
const REACH: f32 = 28.0;

pub fn plugin(app: &mut App) {
    app.add_systems(PostUpdate, shade.after(UiSystems::Layout));
}

/// An end of the scroll area beside it, faded while there is more
/// past it.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
#[require(BackgroundGradient, Pickable = Pickable::IGNORE)]
enum End {
    Top,
    Bottom,
}

/// `area`, a scroll area, with its ends fading where there is more to
/// scroll. It takes the height left, as the area did.
pub fn faded(area: impl View) -> impl View {
    column((area, end(End::Top), end(End::Bottom)))
        .gap(0.0)
        .fills_height()
}

fn end(end: End) -> impl View {
    let (top, bottom) = match end {
        End::Top => (px(0.0), Val::Auto),
        End::Bottom => (Val::Auto, px(0.0)),
    };
    frame()
        .position(PositionType::Absolute)
        .inset(UiRect::new(px(0.0), px(0.0), top, bottom))
        .height(px(REACH))
        .tagged(end)
}

/// Fades each end into the first opaque fill behind its area, as
/// strongly as there is more to scroll past it, up to a whole reach.
fn shade(
    mut ends: Query<(&End, &ChildOf, &mut BackgroundGradient)>,
    areas: Query<&ComputedNode, With<ScrollGoal>>,
    children: Query<&Children>,
    parents: Query<&ChildOf>,
    fills: Query<&BackgroundColor>,
) {
    for (end, parent, mut gradient) in &mut ends {
        let parent = parent.parent();
        let area = children
            .get(parent)
            .into_iter()
            .flatten()
            .find_map(|&child| areas.get(child).ok());
        let ground = core::iter::once(parent)
            .chain(parents.iter_ancestors(parent))
            .filter_map(|entity| fills.get(entity).ok())
            .map(|fill| fill.0)
            .find(|fill| fill.alpha() >= 0.99);
        let (Some(area), Some(ground)) = (area, ground) else {
            gradient.set_if_neq(BackgroundGradient::default());
            continue;
        };
        let reach = REACH / area.inverse_scale_factor();
        let visible = area.size().y - area.scrollbar_size.y;
        let scrolled = area.scroll_position.y;
        let past = match end {
            End::Top => scrolled,
            End::Bottom => area.content_size().y - visible - scrolled,
        };
        let shown = (past / reach).clamp(0.0, 1.0);
        gradient.set_if_neq(fade(*end, ground, shown));
    }
}

/// A gradient from `ground` at `shown` opacity at the end to clear.
fn fade(end: End, ground: Color, shown: f32) -> BackgroundGradient {
    if shown <= 0.0 {
        return BackgroundGradient::default();
    }
    let stops = vec![
        ColorStop::new(ground.with_alpha(shown), percent(0.0)),
        ColorStop::new(ground.with_alpha(0.0), percent(100.0)),
    ];
    BackgroundGradient::from(match end {
        End::Top => LinearGradient::to_bottom(stops),
        End::Bottom => LinearGradient::to_top(stops),
    })
}
