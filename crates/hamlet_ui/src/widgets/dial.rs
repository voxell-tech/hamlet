//! The dial: a ring with a knob that is dragged around it to choose
//! an amount that comes round again, as an hour of the day or a
//! bearing does.

use std::f32::consts::TAU;

use bevy::ecs::component::Component;
use bevy::ecs::event::EntityEvent;
use bevy::ecs::observer::On;
use bevy::ecs::system::{Commands, Query};
use bevy::ecs::world::World;
use bevy::math::Vec2;
use bevy::picking::events::PointerDrag;
use bevy::picking::pointer::PointerButton;
use bevy::ui::{
    AlignItems, ComputedNode, JustifyContent, PositionType,
    UiGlobalTransform, UiRect, Val, px,
};
use bevy::window::SystemCursorIcon;
use bevy_fynix::views::{frame, label};
use bevy_fynix::{EntityCursor, Prop, component};

use crate::{AnyView, Cx, View};

/// A handler run with the world and the amount the dial was asked to
/// take.
type Change = Box<dyn Fn(&mut World, f32) + Send + Sync>;

/// What a dial's face shows: the amount in one whole turn, the
/// amount at the top and how an amount is written in its middle.
#[derive(Component, Clone, Copy)]
pub struct Face {
    pub whole: f32,
    pub top: f32,
    pub text: fn(f32) -> String,
}

impl Face {
    /// Hours of the day: noon at the top, midnight at the foot.
    pub const HOURS: Face = Face {
        whole: 24.0,
        top: 12.0,
        text: |hour| {
            let minutes = (hour * 60.0) as u32;
            format!("{:02}:{:02}", minutes / 60 % 24, minutes % 60)
        },
    };

    /// Degrees of a bearing, nought at the top.
    pub const DEGREES: Face = Face {
        whole: 360.0,
        top: 0.0,
        text: |degrees| format!("{:.0}°", degrees),
    };

    /// The turn from the top, in radians clockwise, at `amount`.
    fn turn(self, amount: f32) -> f32 {
        (amount - self.top) / self.whole * TAU
    }
}

/// On a dial's ring: what runs when it is turned.
#[derive(Component)]
struct Turns(Option<Change>);

/// On a dial's ring: the amount it shows, from 0 to a whole turn.
#[derive(Component, Clone, Copy, PartialEq)]
struct Shown(f32);

/// The side of the ring and of its knob.
const SIDE: f32 = 112.0;
const KNOB: f32 = 16.0;

/// A dial showing `amount` on `face`. It never turns itself: a drag
/// around it hands the amount turned to to `change`, and a bound
/// `amount` follows what that writes. The knob turns by as much as
/// the pointer goes round the middle, so it never jumps.
pub fn dial(
    face: Face,
    amount: impl Into<Prop<f32>>,
    change: impl Fn(&mut World, f32) + Send + Sync + 'static,
) -> impl View {
    let amount = amount.into();
    AnyView::new(move |cx: &mut Cx| {
        let theme = cx.theme();
        let (palette, border) = (&theme.palette, theme.space.border);
        let (rim, knob, middle) =
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
                .fill(middle)
                .align(AlignItems::Center)
                .justify(JustifyContent::Center),
        );
        cx.world
            .entity_mut(ring)
            .insert((
                face,
                Turns(Some(Box::new(change))),
                Shown(0.0),
                EntityCursor(SystemCursorIcon::Grab),
            ))
            .observe(dragged);
        cx.effect(ring, amount, move |world, node, &amount| {
            let to = Shown(amount.rem_euclid(face.whole));
            if let Ok(mut entity) = world.get_entity_mut(node)
                && entity.get::<Shown>() != Some(&to)
            {
                entity.insert(to);
            }
        });
        cx.under(ring, |cx| {
            cx.build(label(component::<Shown, _>(
                ring,
                move |shown| (face.text)(shown.map_or(0.0, |s| s.0)),
            )));
            cx.build(
                frame()
                    .position(PositionType::Absolute)
                    .inset(component::<Shown, _>(
                        ring,
                        move |shown| {
                            let turn =
                                face.turn(shown.map_or(0.0, |s| s.0));
                            let at = place(turn, inner);
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

/// The middle of the knob `turn` radians clockwise from the top, from
/// the top left corner of a ring `inner` across inside its rim.
fn place(turn: f32, inner: f32) -> Vec2 {
    let reach = (inner - KNOB) / 2.0;
    Vec2::splat(inner / 2.0)
        + Vec2::new(turn.sin(), -turn.cos()) * reach
}

/// The radians clockwise from straight up of a point `at` from the
/// middle, in the window's logical pixels, whose y grows downward.
fn bearing(at: Vec2) -> f32 {
    at.x.atan2(-at.y)
}

/// The radians a pointer turned round the middle going from `from`
/// to `to`, the short way, clockwise positive.
fn swept(from: Vec2, to: Vec2) -> f32 {
    let d = bearing(to) - bearing(from);
    (d + TAU / 2.0).rem_euclid(TAU) - TAU / 2.0
}

fn dragged(
    mut drag: On<PointerDrag>,
    rings: Query<(&Face, &Shown, &ComputedNode, &UiGlobalTransform)>,
    mut commands: Commands,
) {
    if drag.button != PointerButton::Primary {
        return;
    }
    drag.propagate(false);
    let node = drag.event_target();
    let Ok((face, shown, computed, transform)) = rings.get(node)
    else {
        return;
    };
    let (_, _, centre) = transform.to_scale_angle_translation();
    let to = drag.pointer.position
        - centre * computed.inverse_scale_factor();
    let turn = swept(to - drag.delta, to);
    let amount =
        (shown.0 + turn / TAU * face.whole).rem_euclid(face.whole);
    commands.queue(move |world: &mut World| {
        let Some(handler) = world
            .get_mut::<Turns>(node)
            .and_then(|mut turns| turns.0.take())
        else {
            return;
        };
        if let Some(mut shown) = world.get_mut::<Shown>(node) {
            shown.0 = amount;
        }
        handler(world, amount);
        if let Some(mut turns) = world.get_mut::<Turns>(node) {
            turns.0 = Some(handler);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drag_turns_by_what_it_sweeps_even_across_the_foot() {
        // A quarter turn clockwise from the top.
        let quarter =
            swept(Vec2::new(0.0, -1.0), Vec2::new(1.0, 0.0));
        assert!((quarter - TAU / 4.0).abs() < 1e-4);
        // Across the foot, where the bearing wraps, it stays small.
        let small = swept(Vec2::new(0.1, 1.0), Vec2::new(-0.1, 1.0));
        assert!(small > 0.0 && small < 0.3);
        let top = place(Face::HOURS.turn(12.0), SIDE);
        assert!((top.x - SIDE / 2.0).abs() < 1e-3);
        assert!((top.y - KNOB / 2.0).abs() < 1e-3);
    }
}
