//! How the switcher moves: the shown tab's indicator gliding to it,
//! a tab dragged along the track with the others easing aside, the
//! track scrolled to the shown tab, and the chevron shown while the
//! tabs do not fit.

use bevy::prelude::*;
use bevy::ui::{ComputedNode, ScrollPosition, UiGlobalTransform};
use bevy_fynix::scroll::{ScrollGoal, wheel};
use bevy_fynix::tokens::{Motion, MotionTokens};
use bevy_fynix::{Dragging, ReducedMotion};

use crate::style::ease;
use crate::switcher::{
    Indicator, MoreButton, TabRow, TabStrip, WorkspaceTab,
};
use crate::theme::EngineTheme;
use crate::workspace::{WorkspaceTuning, Workspaces};

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Glide>()
        .init_resource::<TabDrag>()
        .add_observer(drag_start)
        .add_observer(drag)
        .add_observer(drag_end)
        .add_observer(wheel_across)
        .add_systems(
            Update,
            (preview, glide, reveal, show_more).chain(),
        );
}

/// Where the indicator is drawn across its row, as its left edge and
/// its width: travelling `from` one place `to` another.
#[derive(Resource, Default)]
struct Glide {
    from: Vec2,
    to: Vec2,
    elapsed: f32,
    placed: bool,
}

/// A tab being dragged, and where every tab was and is moved to.
#[derive(Resource, Default)]
pub(crate) struct TabDrag {
    held: Option<Held>,
    /// Each tab's place across its row as the drag began.
    rest: Vec<Vec2>,
    /// How far each tab is moved from its place.
    shift: Vec<f32>,
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
    pub(crate) fn moving(&self) -> bool {
        self.held.is_some_and(|held| held.started)
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

/// The left edge of `node` from that of `within`, and its width, in
/// logical pixels.
fn span(
    node: (&ComputedNode, &UiGlobalTransform),
    within: (&ComputedNode, &UiGlobalTransform),
) -> Vec2 {
    let left =
        |(computed, at): (&ComputedNode, &UiGlobalTransform)| {
            at.translation.x - computed.size().x / 2.0
        };
    let scale = node.0.inverse_scale_factor();
    Vec2::new(
        (left(node) - left(within)) * scale,
        node.0.size().x * scale,
    )
}

type Laid<'a> = (&'a ComputedNode, &'a UiGlobalTransform);

fn drag_start(
    start: On<Pointer<DragStart>>,
    tabs: Query<(&WorkspaceTab, &ComputedNode, &UiGlobalTransform)>,
    rows: Query<Laid, With<TabRow>>,
    mut held: ResMut<TabDrag>,
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
    *held = TabDrag {
        held: Some(Held {
            from: tab.0,
            moved: 0.0,
            started: false,
        }),
        shift: vec![0.0; rest.len()],
        rest,
    };
}

fn drag(
    drag: On<Pointer<Drag>>,
    tabs: Query<&WorkspaceTab>,
    tuning: Res<WorkspaceTuning>,
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
    held.started |= held.moved.abs() >= tuning.drag_start;
}

/// Sets the held tab down where it was dragged to.
fn drag_end(
    end: On<Pointer<DragEnd>>,
    tabs: Query<&WorkspaceTab>,
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
    let to = dragged.slot(held);
    if to != held.from {
        // The tabs are built again in their new order.
        dragged.rest.clear();
        dragged.shift.clear();
        commands.queue(move |world: &mut World| {
            world.resource_mut::<Workspaces>().move_to(held.from, to);
        });
    }
}

/// Moves the held tab with the pointer and eases the others aside to
/// leave a gap where it would be set down, and back once it is let
/// go.
fn preview(
    time: Res<Time>,
    theme: Res<EngineTheme>,
    reduced: Res<ReducedMotion>,
    mut dragged: ResMut<TabDrag>,
    mut tabs: Query<(
        Entity,
        &WorkspaceTab,
        &mut Node,
        Has<Dragging>,
    )>,
    mut commands: Commands,
) {
    if dragged.rest.is_empty() {
        return;
    }
    let rate = theme.motion.interact.recip();
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
            ease(now, want, rate * 2.0, dt)
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
            commands.entity(node).insert((Dragging, ZIndex(1)));
        } else if !lift && lifted {
            commands.entity(node).remove::<(Dragging, ZIndex)>();
        }
    }
    if settled && dragged.held.is_none() {
        dragged.rest.clear();
        dragged.shift.clear();
    }
}

/// Glides the indicator to the shown tab over the theme's motion for
/// a control answering the hand, at once under reduced motion or
/// while a tab is dragged.
fn glide(
    time: Res<Time>,
    theme: Res<EngineTheme>,
    reduced: Res<ReducedMotion>,
    workspaces: Res<Workspaces>,
    dragged: Res<TabDrag>,
    tabs: Query<(&WorkspaceTab, &ComputedNode, &UiGlobalTransform)>,
    rows: Query<Laid, With<TabRow>>,
    mut indicators: Query<&mut Node, With<Indicator>>,
    mut glide: ResMut<Glide>,
) {
    let Ok(mut node) = indicators.single_mut() else {
        return;
    };
    let Ok(row) = rows.single() else {
        return;
    };
    let target = tabs
        .iter()
        .find(|(tab, ..)| tab.0 == workspaces.shown())
        .map(|(_, computed, at)| span((computed, at), row))
        .filter(|span| span.y > 0.0);
    let Some(target) = target else {
        return;
    };
    let curve = theme.motion(Motion::Interact);
    let duration = curve.duration.as_secs_f32();
    let now = |glide: &Glide| {
        let progress = if duration > 0.0 {
            (glide.elapsed / duration).min(1.0)
        } else {
            1.0
        };
        glide.from.lerp(glide.to, (curve.ease)(progress))
    };
    if !glide.placed || reduced.0 || dragged.moving() {
        *glide = Glide {
            from: target,
            to: target,
            elapsed: duration,
            placed: true,
        };
    } else if target != glide.to {
        *glide = Glide {
            from: now(&glide),
            to: target,
            elapsed: 0.0,
            placed: true,
        };
    }
    glide.elapsed += time.delta_secs();
    let at = now(&glide);
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
    workspaces: Res<Workspaces>,
    tabs: Query<(&WorkspaceTab, &ComputedNode, &UiGlobalTransform)>,
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
    if workspaces.is_changed() && *last != Some(workspaces.shown()) {
        *last = Some(workspaces.shown());
        *pending = Some(workspaces.shown());
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
    let scale = computed.inverse_scale_factor();
    let width = computed.size().x * scale;
    let most = ((computed.content_size().x - computed.size().x)
        * scale)
        .max(0.0);
    let delta = if tab.x < 0.0 {
        tab.x
    } else if tab.x + tab.y > width {
        tab.x + tab.y - width
    } else {
        return;
    };
    goal.scroll_by(
        &mut position,
        Vec2::new(delta, 0.0),
        Vec2::new(most, 0.0),
        true,
    );
}

/// Shows the chevron that lists every workspace while the tabs do
/// not fit their track.
fn show_more(
    strips: Query<&ComputedNode, With<TabStrip>>,
    mut more: Query<(&ComputedNode, &mut Node), With<MoreButton>>,
) {
    let (Ok(strip), Ok((button, mut node))) =
        (strips.single(), more.single_mut())
    else {
        return;
    };
    let shown = node.display == Display::Flex;
    // Shown, it takes room from the strip, which is given back as it
    // goes.
    let room =
        strip.size().x + if shown { button.size().x } else { 0.0 };
    let want = if strip.content_size().x > room + 0.5 {
        Display::Flex
    } else {
        Display::None
    };
    if node.display != want {
        node.display = want;
    }
}

/// Scrolls the track across with a wheel turned up or down.
fn wheel_across(
    mut scroll: On<Pointer<Scroll>>,
    mut strips: Query<
        (&ComputedNode, &mut ScrollPosition, &mut ScrollGoal),
        With<TabStrip>,
    >,
) {
    let Ok((computed, mut position, mut goal)) =
        strips.get_mut(scroll.event_target())
    else {
        return;
    };
    let (turned, notched) = wheel(&scroll);
    if turned.y == 0.0 || turned.x != 0.0 {
        return;
    }
    let most = ((computed.content_size().x - computed.size().x)
        * computed.inverse_scale_factor())
    .max(0.0);
    goal.scroll_by(
        &mut position,
        Vec2::new(-turned.y, 0.0),
        Vec2::new(most, 0.0),
        notched,
    );
    scroll.propagate(false);
}
