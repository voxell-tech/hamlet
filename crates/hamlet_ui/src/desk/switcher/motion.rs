//! How the switcher moves: the raised segment gliding to the shown
//! tab, a tab dragged along the track with the others easing aside,
//! and the track scrolled to the shown tab and across by the wheel.
//! The segment and the tabs ease at the rate the dock's hint glides
//! at; the track scrolls as fynix eases a scroll.

use bevy::app::{App, Last, Update};
use bevy::ecs::entity::Entity;
use bevy::ecs::event::EntityEvent;
use bevy::ecs::observer::On;
use bevy::ecs::query::{Has, With};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Local, Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::input::mouse::MouseScrollPixelsPerLine;
use bevy::math::Vec2;
use bevy::picking::events::{
    PointerDrag, PointerDragEnd, PointerDragStart, PointerScroll,
};
use bevy::picking::pointer::PointerButton;
use bevy::time::Time;
use bevy::ui::{
    ComputedNode, Display, Node, ScrollPosition, UiGlobalTransform,
    ZIndex, px,
};
use bevy_fynix::scroll::{ScrollGoal, wheel};
use bevy_fynix::{Dragging, ReducedMotion};

use super::{DeskTab, Indicator, TabRow, TabStrip};
use crate::desk::Desks;
use crate::feel::FeelSettings;
use crate::theme::Themed;

/// How far, in logical pixels, a tab is dragged before it moves.
const DRAG_START: f32 = 4.0;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Glide>()
        .init_resource::<TabDrag>()
        .add_observer(drag_start)
        .add_observer(drag)
        .add_observer(drag_end)
        .add_observer(wheel_across)
        .add_systems(Update, (preview, glide, reveal).chain())
        .add_systems(Last, |mut dragged: ResMut<TabDrag>| {
            dragged.dropped = false;
        });
}

/// Where the raised segment is drawn across its row, as its left
/// edge and its width; none before it is first placed.
#[derive(Resource, Default)]
struct Glide(Option<Vec2>);

/// A tab being dragged, and where every tab was and is moved to.
#[derive(Resource, Default)]
pub(super) struct TabDrag {
    held: Option<Held>,
    /// Each tab's place across its row as the drag began.
    rest: Vec<Vec2>,
    /// How far each tab is moved from its place.
    shift: Vec<f32>,
    /// A drag ended this frame.
    dropped: bool,
}

/// The tab held, how far it has been dragged, and whether that is
/// far enough to move it.
#[derive(Clone, Copy)]
struct Held {
    from: usize,
    moved: f32,
    started: bool,
}

impl TabDrag {
    /// Whether a tab is being moved by a drag.
    pub(super) fn is_moving(&self) -> bool {
        self.held.is_some_and(|held| held.started)
    }

    /// Whether a drag that moved a tab ended this frame.
    pub(super) fn has_dropped(&self) -> bool {
        self.dropped
    }

    /// The place among the others the held tab would be set down at.
    fn slot(&self, held: Held) -> usize {
        let Some(from) = self.rest.get(held.from) else {
            return held.from;
        };
        let middle = from.x + from.y / 2.0 + held.moved;
        self.rest
            .iter()
            .enumerate()
            .filter(|(at, span)| {
                *at != held.from && span.x + span.y / 2.0 < middle
            })
            .count()
    }
}

type Laid<'a> = (&'a ComputedNode, &'a UiGlobalTransform);

/// The left edge of `node` from that of `within`, and its width, in
/// logical pixels.
fn span(node: Laid, within: Laid) -> Vec2 {
    let left = |(computed, at): Laid| {
        at.translation.x - computed.size().x / 2.0
    };
    let scale = node.0.inverse_scale_factor();
    Vec2::new(
        (left(node) - left(within)) * scale,
        node.0.size().x * scale,
    )
}

/// `now` eased toward `want` at `rate` per second over `dt`.
fn ease(now: f32, want: f32, rate: f32, dt: f32) -> f32 {
    let next = now + (want - now) * (1.0 - (-rate * dt).exp());
    // Close enough to stop, so that the ease ends.
    if (want - next).abs() < 0.25 {
        want
    } else {
        next
    }
}

fn drag_start(
    start: On<PointerDragStart>,
    tabs: Query<(&DeskTab, &ComputedNode, &UiGlobalTransform)>,
    rows: Query<Laid, With<TabRow>>,
    mut dragged: ResMut<TabDrag>,
) {
    let Ok((tab, ..)) = tabs.get(start.event_target()) else {
        return;
    };
    let Ok(row) = rows.single() else {
        return;
    };
    if start.button != PointerButton::Primary {
        return;
    }
    let mut rest = tabs
        .iter()
        .map(|(tab, computed, at)| (tab.0, span((computed, at), row)))
        .collect::<Vec<_>>();
    rest.sort_by_key(|(at, _)| *at);
    let rest =
        rest.into_iter().map(|(_, span)| span).collect::<Vec<_>>();
    *dragged = TabDrag {
        held: Some(Held {
            from: tab.0,
            moved: 0.0,
            started: false,
        }),
        shift: vec![0.0; rest.len()],
        rest,
        dropped: false,
    };
}

fn drag(
    drag: On<PointerDrag>,
    tabs: Query<&DeskTab>,
    mut dragged: ResMut<TabDrag>,
) {
    let Ok(tab) = tabs.get(drag.event_target()) else {
        return;
    };
    let Some(held) =
        dragged.held.as_mut().filter(|held| held.from == tab.0)
    else {
        return;
    };
    held.moved = drag.distance.x;
    held.started |= held.moved.abs() >= DRAG_START;
}

/// Sets the held tab down where it was dragged to, and shows it.
fn drag_end(
    end: On<PointerDragEnd>,
    tabs: Query<&DeskTab>,
    mut dragged: ResMut<TabDrag>,
    mut commands: Commands,
) {
    if !tabs.contains(end.event_target()) {
        return;
    }
    let Some(held) = dragged.held.take() else {
        return;
    };
    if !held.started {
        return;
    }
    dragged.dropped = true;
    let to = dragged.slot(held);
    if to != held.from {
        // The tabs are built again in their new order.
        dragged.rest.clear();
        dragged.shift.clear();
    }
    commands.queue(move |world: &mut World| {
        let mut desks = world.resource_mut::<Desks>();
        desks.move_to(held.from, to);
        desks.show(to);
    });
}

/// Moves the held tab with the pointer and eases the others aside to
/// leave a gap where it would be set down, and back once it is let
/// go.
fn preview(
    time: Res<Time>,
    theme: Themed,
    feel: Res<FeelSettings>,
    reduced: Res<ReducedMotion>,
    mut dragged: ResMut<TabDrag>,
    mut tabs: Query<(Entity, &DeskTab, &mut Node, Has<Dragging>)>,
    mut commands: Commands,
) {
    if dragged.rest.is_empty() {
        return;
    }
    let dt = time.delta_secs();
    let held = dragged.held.filter(|held| held.started);
    let slot = held.map(|held| dragged.slot(held));
    let count = dragged.rest.len();
    let mut settled = true;
    for at in 0..count {
        let want = match (held, slot) {
            (Some(held), _) if at == held.from => {
                let span = dragged.rest[at];
                let last = dragged.rest[count - 1];
                held.moved
                    .clamp(-span.x, last.x + last.y - span.x - span.y)
            }
            (Some(held), Some(slot)) => {
                // The tabs are a track's padding apart.
                let room = dragged.rest[held.from].y
                    + theme.space.track_padding;
                if held.from < at && at <= slot {
                    -room
                } else if slot <= at && at < held.from {
                    room
                } else {
                    0.0
                }
            }
            _ => 0.0,
        };
        let now = dragged.shift[at];
        let next = if reduced.0
            || held.is_some_and(|held| held.from == at)
        {
            want
        } else {
            ease(now, want, feel.glide, dt)
        };
        dragged.shift[at] = next;
        settled &= next == 0.0;
    }
    for (node, tab, mut laid, lifted) in &mut tabs {
        let Some(&shift) = dragged.shift.get(tab.0) else {
            continue;
        };
        if laid.left != px(shift) {
            laid.left = px(shift);
        }
        let lift = held.is_some_and(|held| held.from == tab.0);
        if lift && !lifted {
            commands.entity(node).try_insert((Dragging, ZIndex(1)));
        } else if !lift && lifted {
            commands.entity(node).try_remove::<(Dragging, ZIndex)>();
        }
    }
    if settled && dragged.held.is_none() {
        dragged.rest.clear();
        dragged.shift.clear();
    }
}

/// Glides the raised segment to the shown tab, at once when first
/// placed, under reduced motion or while a tab is dragged.
fn glide(
    time: Res<Time>,
    feel: Res<FeelSettings>,
    reduced: Res<ReducedMotion>,
    desks: Res<Desks>,
    dragged: Res<TabDrag>,
    tabs: Query<(&DeskTab, &ComputedNode, &UiGlobalTransform)>,
    rows: Query<Laid, With<TabRow>>,
    mut indicators: Query<&mut Node, With<Indicator>>,
    mut glide: ResMut<Glide>,
) {
    let (Ok(mut node), Ok(row)) =
        (indicators.single_mut(), rows.single())
    else {
        return;
    };
    let target = tabs
        .iter()
        .find(|(tab, ..)| tab.0 == desks.shown())
        .map(|(_, computed, at)| span((computed, at), row))
        .filter(|span| span.y > 0.0);
    let Some(target) = target else {
        return;
    };
    let dt = time.delta_secs();
    let at = match glide.0 {
        Some(was) if !reduced.0 && !dragged.is_moving() => Vec2::new(
            ease(was.x, target.x, feel.glide, dt),
            ease(was.y, target.y, feel.glide, dt),
        ),
        _ => target,
    };
    glide.0 = Some(at);
    if node.left != px(at.x) || node.width != px(at.y) {
        node.left = px(at.x);
        node.width = px(at.y);
    }
    if node.display != Display::Flex {
        node.display = Display::Flex;
    }
}

/// Scrolls the track to the shown tab once it is laid out, each time
/// another is shown.
fn reveal(
    desks: Res<Desks>,
    tabs: Query<(&DeskTab, &ComputedNode, &UiGlobalTransform)>,
    mut strips: Query<
        (
            &ComputedNode,
            &UiGlobalTransform,
            &mut ScrollPosition,
            &mut ScrollGoal,
        ),
        With<TabStrip>,
    >,
    mut pending: Local<Option<usize>>,
    mut last: Local<Option<usize>>,
) {
    if *last != Some(desks.shown()) {
        *last = Some(desks.shown());
        *pending = Some(desks.shown());
    }
    let Some(shown) = *pending else {
        return;
    };
    let Ok((computed, at, mut position, mut goal)) =
        strips.single_mut()
    else {
        return;
    };
    let Some(tab) = tabs
        .iter()
        .find(|(tab, ..)| tab.0 == shown)
        .map(|(_, tab, place)| span((tab, place), (computed, at)))
        .filter(|span| span.y > 0.0)
    else {
        return;
    };
    *pending = None;
    let width = computed.size().x * computed.inverse_scale_factor();
    let by = if tab.x < 0.0 {
        tab.x
    } else if tab.x + tab.y > width {
        tab.x + tab.y - width
    } else {
        return;
    };
    goal.scroll_by(
        &mut position,
        Vec2::new(by, 0.0),
        Vec2::new(most(computed), 0.0),
        true,
    );
}

/// The furthest the track scrolls across, in logical pixels.
fn most(computed: &ComputedNode) -> f32 {
    ((computed.content_size().x - computed.size().x)
        * computed.inverse_scale_factor())
    .max(0.0)
}

/// Scrolls the track across with a wheel turned up or down. A wheel
/// turned across is fynix's to scroll.
fn wheel_across(
    mut scroll: On<PointerScroll>,
    per_line: Res<MouseScrollPixelsPerLine>,
    mut strips: Query<
        (&ComputedNode, &mut ScrollPosition, &mut ScrollGoal),
        With<TabStrip>,
    >,
) {
    let Ok((computed, mut position, mut goal)) =
        strips.get_mut(scroll.entity)
    else {
        return;
    };
    let (turned, notched) = wheel(&scroll, *per_line);
    if turned.y == 0.0 || turned.x != 0.0 {
        return;
    }
    goal.scroll_by(
        &mut position,
        Vec2::new(-turned.y, 0.0),
        Vec2::new(most(computed), 0.0),
        notched,
    );
    scroll.propagate(false);
}
