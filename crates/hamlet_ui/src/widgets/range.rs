//! A range: a track with two thumbs dragged along it to choose the
//! least and the most of an amount, the groove filled between them.

use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::event::EntityEvent;
use bevy::ecs::observer::On;
use bevy::ecs::system::{Commands, Query};
use bevy::ecs::world::World;
use bevy::picking::events::{PointerDrag, PointerPress};
use bevy::picking::pointer::PointerButton;
use bevy::ui::{
    AlignItems, ComputedNode, PositionType, UiGlobalTransform,
    UiRect, Val, percent, px,
};
use bevy::window::SystemCursorIcon;
use bevy_fynix::tokens::{SpacingTokens, SurfaceTokens};
use bevy_fynix::views::frame;
use bevy_fynix::{EntityCursor, Prop, component};

use crate::{AnyView, Cx, View};

/// A handler run with the world and the least and the most the range
/// was asked to take.
type Change = Box<dyn Fn(&mut World, f32, f32) + Send + Sync>;

/// On a range's track: its ends, how wide its thumbs are and what
/// runs when one is slid.
#[derive(Component)]
struct Ends {
    range: (f32, f32),
    thumb: f32,
    on_change: Option<Change>,
}

/// On a range's track: how far along its least and its most are,
/// each from 0 to 1.
#[derive(Component, Clone, Copy, PartialEq, Default)]
struct Shown {
    low: f32,
    high: f32,
}

/// On a range's track while it is held: which thumb follows the
/// pointer.
#[derive(Component, Clone, Copy, PartialEq, Debug)]
enum Holding {
    Low,
    High,
}

/// A range from `least` to `most` showing `low` and `high`. It never
/// moves itself: a press takes the thumb nearer the pointer to it, a
/// drag carries that thumb along, and each calls `change` with the
/// least and the most asked for, never crossed. Bound `low` and
/// `high` follow what that writes to the world.
pub fn range(
    low: impl Into<Prop<f32>>,
    high: impl Into<Prop<f32>>,
    (least, most): (f32, f32),
    change: impl Fn(&mut World, f32, f32) + Send + Sync + 'static,
) -> impl View {
    let (low, high) = (low.into(), high.into());
    AnyView::new(move |cx: &mut Cx| {
        let theme = cx.theme();
        let row = theme.row();
        let groove = (row / 7.0).round().max(2.0);
        let thumb = ((row * 0.36).round(), (row * 0.64).round());
        let (well, accent, edge) =
            (theme.fill(), theme.accent(), theme.hairline());
        let span = most - least;
        let share = move |value: f32| {
            if span == 0.0 {
                0.0
            } else {
                ((value - least) / span).clamp(0.0, 1.0)
            }
        };
        let track = cx.build(
            frame()
                .width(percent(100.0))
                .min_width(px(0.0))
                .grow(1.0)
                .height(px(row))
                .align(AlignItems::Center),
        );
        cx.world
            .entity_mut(track)
            .insert((
                Ends {
                    range: (least, most),
                    thumb: thumb.0,
                    on_change: Some(Box::new(change)),
                },
                Shown::default(),
                EntityCursor(SystemCursorIcon::Pointer),
            ))
            .observe(pressed)
            .observe(dragged);
        cx.effect(track, low, move |world, node, &value| {
            show(world, node, |shown| shown.low = share(value));
        });
        cx.effect(track, high, move |world, node, &value| {
            show(world, node, |shown| shown.high = share(value));
        });
        let middle = move |tall: f32| px((row - tall) / 2.0);
        cx.under(track, |cx| {
            cx.build(
                frame()
                    .width(percent(100.0))
                    .height(px(groove))
                    .radius(groove / 2.0)
                    .fill(well)
                    .border(1.0)
                    .border_color(edge),
            );
            // The fill runs between the thumbs' middles, which travel
            // the track less half a thumb at either end.
            let between = cx.build(
                frame().position(PositionType::Absolute).inset(
                    UiRect {
                        left: px(thumb.0 / 2.0),
                        right: px(thumb.0 / 2.0),
                        ..UiRect::all(px(0.0))
                    },
                ),
            );
            cx.under(between, |cx| {
                cx.build(
                    frame()
                        .position(PositionType::Absolute)
                        .inset(component::<Shown, _>(
                            track,
                            move |shown| UiRect {
                                left: percent(
                                    shown.map_or(0.0, |s| s.low)
                                        * 100.0,
                                ),
                                top: middle(groove),
                                ..UiRect::all(Val::Auto)
                            },
                        ))
                        .width(component::<Shown, _>(
                            track,
                            |shown| {
                                percent(shown.map_or(0.0, |s| {
                                    (s.high - s.low).max(0.0) * 100.0
                                }))
                            },
                        ))
                        .height(px(groove))
                        .radius(groove / 2.0)
                        .fill(accent),
                );
            });
            // The thumbs travel the track less their own width.
            let travel = cx.build(
                frame().position(PositionType::Absolute).inset(
                    UiRect {
                        right: px(thumb.0),
                        ..UiRect::all(px(0.0))
                    },
                ),
            );
            cx.under(travel, |cx| {
                for end in [Holding::Low, Holding::High] {
                    cx.build(
                        frame()
                            .position(PositionType::Absolute)
                            .inset(component::<Shown, _>(
                                track,
                                move |shown| {
                                    let shown = shown
                                        .copied()
                                        .unwrap_or_default();
                                    let at = match end {
                                        Holding::Low => shown.low,
                                        Holding::High => shown.high,
                                    };
                                    UiRect {
                                        left: percent(at * 100.0),
                                        top: middle(thumb.1),
                                        ..UiRect::all(Val::Auto)
                                    }
                                },
                            ))
                            .width(px(thumb.0))
                            .height(px(thumb.1))
                            .radius(2.0)
                            .fill(accent),
                    );
                }
            });
        });
        track
    })
}

/// Changes what the track `node` shows by `change`, if it differs.
fn show(
    world: &mut World,
    node: Entity,
    change: impl Fn(&mut Shown),
) {
    let Ok(mut entity) = world.get_entity_mut(node) else {
        return;
    };
    let mut to = entity.get::<Shown>().copied().unwrap_or_default();
    change(&mut to);
    if entity.get::<Shown>() != Some(&to) {
        entity.insert(to);
    }
}

/// How far along a track `wide`, whose left edge is at `left`, the
/// middle of a thumb `thumb` wide is when it is at `x`.
fn along(x: f32, left: f32, wide: f32, thumb: f32) -> f32 {
    let travel = (wide - thumb).max(1.0);
    ((x - left - thumb / 2.0) / travel).clamp(0.0, 1.0)
}

/// The thumb a press `at` along a track showing `shown` takes: the
/// nearer, or where both stand together, the one on the side pressed.
fn nearer(shown: Shown, at: f32) -> Holding {
    let (to_low, to_high) =
        ((at - shown.low).abs(), (at - shown.high).abs());
    if to_low < to_high || to_low == to_high && at < shown.low {
        Holding::Low
    } else {
        Holding::High
    }
}

/// The least and the most a track showing `shown` asks for once the
/// thumb `held` is taken to `at`, both as shares of the way along:
/// a thumb stops at the other.
fn moved(shown: Shown, held: Holding, at: f32) -> (f32, f32) {
    match held {
        Holding::Low => (at.min(shown.high), shown.high),
        Holding::High => (shown.low, at.max(shown.low)),
    }
}

/// Takes the thumb `held`, or the nearer one, of the track `node` to
/// the pointer at `x`, in the window's logical pixels, and asks for
/// the least and the most that makes.
fn slide_to(
    node: Entity,
    x: f32,
    held: Option<Holding>,
    tracks: &Query<(
        &Ends,
        &Shown,
        &ComputedNode,
        &UiGlobalTransform,
    )>,
    commands: &mut Commands,
) {
    let Ok((ends, shown, computed, transform)) = tracks.get(node)
    else {
        return;
    };
    let scale = computed.inverse_scale_factor();
    let (_, _, centre) = transform.to_scale_angle_translation();
    let wide = computed.size().x * scale;
    let left = centre.x * scale - wide / 2.0;
    let at = along(x, left, wide, ends.thumb);
    let held = held.unwrap_or_else(|| nearer(*shown, at));
    let (low, high) = moved(*shown, held, at);
    let (least, most) = ends.range;
    let value = move |share: f32| least + (most - least) * share;
    let (low, high) = (value(low), value(high));
    commands.entity(node).insert(held);
    commands.queue(move |world: &mut World| {
        let Some(handler) = world
            .get_mut::<Ends>(node)
            .and_then(|mut ends| ends.on_change.take())
        else {
            return;
        };
        handler(world, low, high);
        if let Some(mut ends) = world.get_mut::<Ends>(node) {
            ends.on_change = Some(handler);
        }
    });
}

fn pressed(
    mut press: On<PointerPress>,
    tracks: Query<(&Ends, &Shown, &ComputedNode, &UiGlobalTransform)>,
    mut commands: Commands,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    press.propagate(false);
    let x = press.pointer.position.x;
    slide_to(press.event_target(), x, None, &tracks, &mut commands);
}

fn dragged(
    mut drag: On<PointerDrag>,
    tracks: Query<(&Ends, &Shown, &ComputedNode, &UiGlobalTransform)>,
    holding: Query<&Holding>,
    mut commands: Commands,
) {
    if drag.button != PointerButton::Primary {
        return;
    }
    drag.propagate(false);
    let node = drag.event_target();
    let x = drag.pointer.position.x;
    let held = holding.get(node).ok().copied();
    slide_to(node, x, held, &tracks, &mut commands);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_press_takes_the_nearer_thumb_and_neither_crosses_the_other()
    {
        let shown = Shown {
            low: 0.2,
            high: 0.6,
        };
        assert_eq!(nearer(shown, 0.1), Holding::Low);
        assert_eq!(nearer(shown, 0.5), Holding::High);
        // Dragged past the other, a thumb stops at it.
        assert_eq!(moved(shown, Holding::Low, 0.9), (0.6, 0.6));
        assert_eq!(moved(shown, Holding::High, 0.0), (0.2, 0.2));
        // Where both stand together, the side pressed decides.
        let together = Shown {
            low: 0.5,
            high: 0.5,
        };
        assert_eq!(nearer(together, 0.3), Holding::Low);
        assert_eq!(nearer(together, 0.7), Holding::High);
    }
}
