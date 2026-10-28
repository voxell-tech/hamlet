//! The dial: a ring with a knob that is dragged around it to choose
//! an hour of the day, noon at the top and midnight at the bottom.

use std::f32::consts::TAU;

use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::event::EntityEvent;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::system::{Commands, Query};
use bevy::ecs::world::World;
use bevy::math::Vec2;
use bevy::picking::events::{PointerDrag, PointerPress};
use bevy::picking::pointer::PointerButton;
use bevy::ui::{
    AlignItems, ComputedNode, JustifyContent, PositionType,
    UiGlobalTransform, UiRect, Val, px,
};
use bevy::window::SystemCursorIcon;
use bevy_fynix::views::{frame, label};
use bevy_fynix::{EntityCursor, Prop, component};

use crate::{AnyView, Cx, View};

/// A handler run with the world and the hour the dial was asked to
/// take.
type Change = Box<dyn Fn(&mut World, f32) + Send + Sync>;

/// On a dial's ring: what runs when it is turned.
#[derive(Component)]
struct Turns(Option<Change>);

/// On a dial's ring: the hour it shows, 0 to 24.
#[derive(Component, Clone, Copy, PartialEq)]
struct Shown(f32);

/// The side of the ring and of its knob.
const SIDE: f32 = 112.0;
const KNOB: f32 = 16.0;

/// A dial showing `hour`. It never turns itself: a press on the ring
/// or a drag around it hands the hour under the pointer to `change`,
/// and a bound `hour` follows what that writes.
pub fn dial(
    hour: impl Into<Prop<f32>>,
    change: impl Fn(&mut World, f32) + Send + Sync + 'static,
) -> impl View {
    let hour = hour.into();
    AnyView::new(move |cx: &mut Cx| {
        let theme = cx.theme();
        let (palette, border) = (&theme.palette, theme.space.border);
        let (rim, knob, face) =
            (palette.brass_3, palette.brass_1, palette.grey_4);
        // The knob is placed inside the rim.
        let inner = SIDE - 4.0 * border;
        let ring = cx.build(
            frame()
                .width(px(SIDE))
                .height(px(SIDE))
                .shrink(0.0)
                .radius(SIDE / 2.0)
                .border(border * 2.0)
                .border_color(rim)
                .fill(face)
                .align(AlignItems::Center)
                .justify(JustifyContent::Center),
        );
        cx.world
            .entity_mut(ring)
            .insert((
                Turns(Some(Box::new(change))),
                Shown(0.0),
                EntityCursor(SystemCursorIcon::Grab),
            ))
            .observe(pressed)
            .observe(dragged);
        cx.effect(ring, hour, |world, node, &hour| {
            let to = Shown(hour.rem_euclid(24.0));
            if let Ok(mut entity) = world.get_entity_mut(node)
                && entity.get::<Shown>() != Some(&to)
            {
                entity.insert(to);
            }
        });
        cx.under(ring, |cx| {
            cx.build(label(component::<Shown, _>(ring, |shown| {
                let minutes =
                    (shown.map_or(0.0, |s| s.0) * 60.0) as u32;
                format!(
                    "{:02}:{:02}",
                    minutes / 60 % 24,
                    minutes % 60
                )
            })));
            cx.build(
                frame()
                    .position(PositionType::Absolute)
                    .inset(component::<Shown, _>(
                        ring,
                        move |shown| {
                            let at = place(
                                shown.map_or(0.0, |s| s.0),
                                inner,
                            );
                            UiRect {
                                left: px(at.x - KNOB / 2.0),
                                top: px(at.y - KNOB / 2.0),
                                ..UiRect::all(Val::Auto)
                            }
                        },
                    ))
                    .width(px(KNOB))
                    .height(px(KNOB))
                    .radius(KNOB / 2.0)
                    .fill(knob),
            );
        });
        ring
    })
}

/// The middle of the knob at `hour`, from the top left corner of a
/// ring `inner` across inside its rim.
fn place(hour: f32, inner: f32) -> Vec2 {
    let turn = (hour / 24.0 - 0.5) * TAU;
    let reach = (inner - KNOB) / 2.0;
    Vec2::splat(inner / 2.0)
        + Vec2::new(turn.sin(), -turn.cos()) * reach
}

/// The hour under a pointer at `at` from the ring's middle, in the
/// window's logical pixels: noon straight up, turning clockwise.
fn hour_at(at: Vec2) -> f32 {
    let turn = at.x.atan2(-at.y);
    (turn / TAU * 24.0 + 12.0).rem_euclid(24.0)
}

/// Asks the dial `node` for the hour under a pointer at `at`.
fn turn_to(
    node: Entity,
    at: Vec2,
    rings: &Query<(&ComputedNode, &UiGlobalTransform), With<Turns>>,
    commands: &mut Commands,
) {
    let Ok((computed, transform)) = rings.get(node) else {
        return;
    };
    let (_, _, centre) = transform.to_scale_angle_translation();
    let hour = hour_at(at - centre * computed.inverse_scale_factor());
    commands.queue(move |world: &mut World| {
        let Some(handler) = world
            .get_mut::<Turns>(node)
            .and_then(|mut turns| turns.0.take())
        else {
            return;
        };
        handler(world, hour);
        if let Some(mut turns) = world.get_mut::<Turns>(node) {
            turns.0 = Some(handler);
        }
    });
}

fn pressed(
    mut press: On<PointerPress>,
    rings: Query<(&ComputedNode, &UiGlobalTransform), With<Turns>>,
    mut commands: Commands,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    press.propagate(false);
    let at = press.pointer.position;
    turn_to(press.event_target(), at, &rings, &mut commands);
}

fn dragged(
    mut drag: On<PointerDrag>,
    rings: Query<(&ComputedNode, &UiGlobalTransform), With<Turns>>,
    mut commands: Commands,
) {
    if drag.button != PointerButton::Primary {
        return;
    }
    drag.propagate(false);
    let at = drag.pointer.position;
    turn_to(drag.event_target(), at, &rings, &mut commands);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noon_is_at_the_top_and_the_hours_turn_clockwise() {
        assert!((hour_at(Vec2::new(0.0, -1.0)) - 12.0).abs() < 1e-4);
        assert!((hour_at(Vec2::new(1.0, 0.0)) - 18.0).abs() < 1e-4);
        assert!((hour_at(Vec2::new(0.0, 1.0)) % 24.0).abs() < 1e-4);
        let top = place(12.0, SIDE);
        assert!((top.x - SIDE / 2.0).abs() < 1e-3);
        assert!((top.y - KNOB / 2.0).abs() < 1e-3);
    }
}
