//! The ends of a scroll area fade into what it sits on while there is
//! more to scroll that way: a gradient laid over each end, from clear
//! to the first opaque fill behind the area. The rows under it stay
//! whole, letters and all, and only darken or lighten together.

use bevy::app::{App, PostUpdate};
use bevy::color::{Alpha, Color, Mix};
use bevy::ecs::change_detection::DetectChangesMut;
use bevy::ecs::component::Component;
use bevy::ecs::hierarchy::{ChildOf, Children};
use bevy::ecs::query::{With, Without};
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::Query;
use bevy::picking::Pickable;
use bevy::ui::{
    BackgroundColor, BackgroundGradient, ColorStop, ComputedNode,
    Gradient, LinearGradient, PositionType, UiGlobalTransform,
    UiRect, UiSystems, Val, percent, px,
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
    Left,
    Right,
}

impl End {
    /// Whether the end runs along the top or the bottom.
    fn across(self) -> bool {
        matches!(self, Self::Top | Self::Bottom)
    }
}

/// `area`, a scroll area, with its ends fading where there is more to
/// scroll, up and down and side to side. It takes the height left,
/// as the area did.
pub fn faded(area: impl View) -> impl View {
    column((
        area,
        end(End::Top),
        end(End::Bottom),
        end(End::Left),
        end(End::Right),
    ))
    .gap(0.0)
    .fills_height()
}

fn end(end: End) -> impl View {
    let (left, right, top, bottom) = match end {
        End::Top => (px(0.0), px(0.0), px(0.0), Val::Auto),
        End::Bottom => (px(0.0), px(0.0), Val::Auto, px(0.0)),
        End::Left => (px(0.0), Val::Auto, px(0.0), px(0.0)),
        End::Right => (Val::Auto, px(0.0), px(0.0), px(0.0)),
    };
    let shown = frame()
        .position(PositionType::Absolute)
        .inset(UiRect::new(left, right, top, bottom));
    if end.across() {
        shown.height(px(REACH)).tagged(end)
    } else {
        shown.width(px(REACH)).tagged(end)
    }
}

/// Fades each end into the first opaque fill behind its area, in the
/// colour that fill has there once its own gradient is laid on it,
/// as strongly as there is more to scroll past the end, up to a
/// whole reach.
fn shade(
    mut ends: Query<(&End, &ChildOf, &mut BackgroundGradient)>,
    areas: Query<
        (&ComputedNode, &UiGlobalTransform),
        With<ScrollGoal>,
    >,
    children: Query<&Children>,
    parents: Query<&ChildOf>,
    fills: Query<
        (
            &BackgroundColor,
            Option<&BackgroundGradient>,
            &ComputedNode,
            &UiGlobalTransform,
        ),
        Without<End>,
    >,
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
            .find(|(fill, ..)| fill.0.alpha() >= 0.99);
        let (Some((area, at)), Some((fill, laid, node, node_at))) =
            (area, ground)
        else {
            gradient.set_if_neq(BackgroundGradient::default());
            continue;
        };
        // How far down the ground's own gradient the end is: a side
        // end takes the colour by the middle of the area.
        let edge = match end {
            End::Top => at.translation.y - area.size().y / 2.0,
            End::Bottom => at.translation.y + area.size().y / 2.0,
            End::Left | End::Right => at.translation.y,
        };
        let along = (edge
            - (node_at.translation.y - node.size().y / 2.0))
            / node.size().y.max(1.0);
        let ground = laid
            .and_then(|laid| colour_at(laid, along))
            .unwrap_or(fill.0);
        let reach = REACH / area.inverse_scale_factor();
        let (visible, scrolled, content) = if end.across() {
            (
                area.size().y - area.scrollbar_size.y,
                area.scroll_position.y,
                area.content_size().y,
            )
        } else {
            (
                area.size().x - area.scrollbar_size.x,
                area.scroll_position.x,
                area.content_size().x,
            )
        };
        let past = match end {
            End::Top | End::Left => scrolled,
            End::Bottom | End::Right => content - visible - scrolled,
        };
        let shown = (past / reach).clamp(0.0, 1.0);
        gradient.set_if_neq(fade(*end, ground, shown));
    }
}

/// The colour of `laid`'s first top-to-bottom gradient at `along`
/// of its node's height, from 0 at the top to 1, or `None` when it
/// has no such gradient.
fn colour_at(laid: &BackgroundGradient, along: f32) -> Option<Color> {
    let Gradient::Linear(line) = laid.0.first()? else {
        return None;
    };
    if (line.angle - LinearGradient::TO_BOTTOM).abs() > 1e-3 {
        return None;
    }
    let at = |stop: &ColorStop| match stop.point {
        Val::Percent(share) => share / 100.0,
        _ => 0.0,
    };
    let along = along.clamp(0.0, 1.0);
    let last = line.stops.last()?;
    line.stops
        .windows(2)
        .find_map(|pair| {
            let (from, to) = (at(&pair[0]), at(&pair[1]));
            (along >= from && along <= to).then(|| {
                let share = (along - from) / (to - from).max(1e-6);
                pair[0].color.mix(&pair[1].color, share)
            })
        })
        .or_else(|| {
            let first = line.stops.first()?;
            Some(if along < at(first) {
                first.color
            } else {
                last.color
            })
        })
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
        End::Left => LinearGradient::to_right(stops),
        End::Right => LinearGradient::to_left(stops),
    })
}
