//! The dial: a ring with a knob that is dragged around it to choose
//! an amount that comes round again, as an hour of the day or a
//! bearing does.

use std::f32::consts::TAU;

use bevy::ecs::component::Component;
use bevy::ecs::event::EntityEvent;
use bevy::ecs::observer::On;
use bevy::ecs::system::{Commands, Query, ResMut};
use bevy::ecs::world::World;
use bevy::math::{Rot2, Vec2};
use bevy::picking::events::{
    PointerDrag, PointerDragEnd, PointerDragStart,
};
use bevy::picking::pointer::PointerButton;
use bevy::ui::widget::ImageNode;
use bevy::ui::{
    AlignItems, ComputedNode, JustifyContent, PositionType,
    UiGlobalTransform, UiRect, UiTransform, Val, px,
};
use bevy::window::SystemCursorIcon;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, frame, icon as icon_view, label,
};
use bevy_fynix::{EntityCursor, OverrideCursor, Prop, component};

use super::icons::Glyph;
use crate::{AnyView, Cx, View};

/// A handler run with the world and the amount the dial was asked to
/// take.
type Change = Box<dyn Fn(&mut World, f32) + Send + Sync>;

/// What a dial's face shows: the amount in one whole turn, the
/// amount at the top, how an amount is written in its middle, the
/// marks round its rim, every so many of them long, and whether the
/// icon in its middle turns with the amount.
#[derive(Component, Clone, Copy)]
pub struct Face {
    pub whole: f32,
    pub top: f32,
    pub text: fn(f32) -> String,
    pub marks: u32,
    pub long: u32,
    pub turns_icon: bool,
}

impl Face {
    /// Hours of the day: noon at the top, midnight at the foot, a
    /// mark each hour and a long one each quarter of the day.
    pub const HOURS: Face = Face {
        whole: 24.0,
        top: 12.0,
        text: |hour| {
            let minutes = (hour * 60.0) as u32;
            format!("{:02}:{:02}", minutes / 60 % 24, minutes % 60)
        },
        marks: 24,
        long: 6,
        turns_icon: false,
    };

    /// Degrees of a bearing, nought at the top, a mark each ten and a
    /// long one each quarter turn. The icon points along it.
    pub const DEGREES: Face = Face {
        whole: 360.0,
        top: 0.0,
        text: |degrees| format!("{:.0}°", degrees),
        marks: 36,
        long: 9,
        turns_icon: true,
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

/// On a dial's ring: the amount that trails after the one it shows,
/// from 0 to a whole turn.
#[derive(Component, Clone, Copy, PartialEq)]
struct Trailed(f32);

/// On a dial's ring while it is dragged: the radians from where the
/// pointer took hold to where the knob was then.
#[derive(Component, Clone, Copy)]
struct Grip(f32);

/// The side of the ring, of its knob, of the dot that trails it and
/// of the icon in its middle.
const SIDE: f32 = 112.0;
const KNOB: f32 = 16.0;
const TRAIL: f32 = 8.0;
const ICON: f32 = 20.0;

/// The length of a short and of a long mark round the rim, and the
/// gap between the marks and the rim.
const MARK: f32 = 4.0;
const LONG_MARK: f32 = 8.0;
const MARK_GAP: f32 = 4.0;

/// A dial showing `amount` on `face`, `icon` and the amount in its
/// middle, and a smaller dot at `trail`, where what it sets has got
/// to. It never turns itself: a drag around it hands the amount
/// turned to to `change`, and a bound `amount` follows what that
/// writes. The knob sits on the rim and keeps the angle to the
/// pointer it had when the drag began, so it never jumps.
pub fn dial(
    face: Face,
    icon: impl Into<Prop<Glyph>>,
    amount: impl Into<Prop<f32>>,
    trail: impl Into<Prop<f32>>,
    change: impl Fn(&mut World, f32) + Send + Sync + 'static,
) -> impl View {
    let (icon, amount, trail) =
        (icon.into(), amount.into(), trail.into());
    AnyView::new(move |cx: &mut Cx| {
        let theme = cx.theme();
        let (palette, border) = (&theme.palette, theme.space.border);
        let (rim, knob, trailing, middle) = (
            palette.gold_3,
            palette.gold_1,
            palette.gold_4,
            palette.button,
        );
        let (short_mark, long_mark) =
            (palette.gold_5, palette.gold_3);
        let rim_width = border * 2.0;
        // Places are measured inside the rim; its line is half its
        // width further out.
        let inner = SIDE - 2.0 * rim_width;
        let on_rim = inner / 2.0 + rim_width / 2.0;
        let ring = cx.build(
            frame()
                .width(px(SIDE))
                .height(px(SIDE))
                .shrink(0.0)
                .radius(SIDE / 2.0)
                .border(rim_width)
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
                Trailed(0.0),
                EntityCursor(SystemCursorIcon::Grab),
            ))
            .observe(gripped)
            .observe(dragged)
            .observe(let_go);
        cx.effect(ring, amount, move |world, node, &amount| {
            let to = Shown(amount.rem_euclid(face.whole));
            if let Ok(mut entity) = world.get_entity_mut(node)
                && entity.get::<Shown>() != Some(&to)
            {
                entity.insert(to);
            }
        });
        cx.effect(ring, trail, move |world, node, &amount| {
            let to = Trailed(amount.rem_euclid(face.whole));
            if let Ok(mut entity) = world.get_entity_mut(node)
                && entity.get::<Trailed>() != Some(&to)
            {
                entity.insert(to);
            }
        });
        // A dot `side` across, on the rim `turn` radians from the
        // top.
        let dot = move |turn: f32, side: f32| {
            let at = place(turn, inner, on_rim);
            UiRect {
                left: px(at.x - side / 2.0),
                top: px(at.y - side / 2.0),
                ..UiRect::all(Val::Auto)
            }
        };
        cx.under(ring, |cx| {
            for k in 0..face.marks {
                let turn = k as f32 / face.marks as f32 * TAU;
                let long = k % face.long == 0;
                let (tall, colour) = if long {
                    (LONG_MARK, long_mark)
                } else {
                    (MARK, short_mark)
                };
                let wide = if long { 2.0 } else { 1.5 };
                let at = place(
                    turn,
                    inner,
                    inner / 2.0 - MARK_GAP - tall / 2.0,
                );
                cx.build(
                    frame()
                        .position(PositionType::Absolute)
                        .inset(UiRect {
                            left: px(at.x - wide / 2.0),
                            top: px(at.y - tall / 2.0),
                            ..UiRect::all(Val::Auto)
                        })
                        .width(px(wide))
                        .height(px(tall))
                        .radius(wide / 2.0)
                        .fill(colour)
                        .tagged(UiTransform::from_rotation(
                            Rot2::radians(turn),
                        )),
                );
            }
            let centre = cx
                .build(column(()).gap(2.0).align(AlignItems::Center));
            cx.under(centre, |cx| {
                let mark = cx.build(
                    icon_view(Glyph::Sun.image(ICON))
                        .size(ICON)
                        .tone(Tone::Dim)
                        .rotation(component::<Shown, _>(
                            ring,
                            move |shown| {
                                if face.turns_icon {
                                    face.turn(
                                        shown.map_or(0.0, |s| s.0),
                                    )
                                    .to_degrees()
                                } else {
                                    0.0
                                }
                            },
                        )),
                );
                cx.effect(
                    mark,
                    icon,
                    |world, node, &glyph: &Glyph| {
                        if let Some(mut image) =
                            world.get_mut::<ImageNode>(node)
                        {
                            let to = glyph.image(ICON);
                            if image.image != to {
                                image.image = to;
                            }
                        }
                    },
                );
                cx.build(
                    label(component::<Shown, _>(
                        ring,
                        move |shown| {
                            (face.text)(shown.map_or(0.0, |s| s.0))
                        },
                    ))
                    .size(cx.theme().text.small),
                );
            });
            cx.build(
                frame()
                    .position(PositionType::Absolute)
                    .inset(component::<Trailed, _>(ring, move |at| {
                        dot(face.turn(at.map_or(0.0, |a| a.0)), TRAIL)
                    }))
                    .width(px(TRAIL))
                    .height(px(TRAIL))
                    .radius(TRAIL / 2.0)
                    .fill(trailing),
            );
            cx.build(
                frame()
                    .position(PositionType::Absolute)
                    .inset(component::<Shown, _>(ring, move |at| {
                        dot(face.turn(at.map_or(0.0, |a| a.0)), KNOB)
                    }))
                    .width(px(KNOB))
                    .height(px(KNOB))
                    .radius(KNOB / 2.0)
                    // A ring of the face's colour sets it off the rim
                    // it sits on.
                    .border(border * 1.5)
                    .border_color(middle)
                    .fill(knob),
            );
        });
        ring
    })
}

/// The point `reach` from the middle, `turn` radians clockwise from
/// the top, from the top left corner of a ring `inner` across inside
/// its rim.
fn place(turn: f32, inner: f32, reach: f32) -> Vec2 {
    Vec2::splat(inner / 2.0)
        + Vec2::new(turn.sin(), -turn.cos()) * reach
}

/// The radians clockwise from straight up of a point `at` from the
/// middle, in the window's logical pixels, whose y grows downward.
fn bearing(at: Vec2) -> f32 {
    at.x.atan2(-at.y)
}

/// The pointer at `at`, in the window's logical pixels, from the
/// middle of the ring laid out as `computed` and `transform`.
fn from_middle(
    at: Vec2,
    computed: &ComputedNode,
    transform: &UiGlobalTransform,
) -> Vec2 {
    let (_, _, centre) = transform.to_scale_angle_translation();
    at - centre * computed.inverse_scale_factor()
}

/// The amount on `face` with the knob `turn` radians from the top.
fn amount_at(face: &Face, turn: f32) -> f32 {
    (face.top + turn / TAU * face.whole).rem_euclid(face.whole)
}

/// Takes hold of the dial, and shows a closed hand until it is let
/// go, wherever the pointer goes meanwhile.
fn gripped(
    mut start: On<PointerDragStart>,
    rings: Query<(&Face, &Shown, &ComputedNode, &UiGlobalTransform)>,
    mut cursor: ResMut<OverrideCursor>,
    mut commands: Commands,
) {
    if start.button != PointerButton::Primary {
        return;
    }
    start.propagate(false);
    let node = start.event_target();
    let Ok((face, shown, computed, transform)) = rings.get(node)
    else {
        return;
    };
    let at = from_middle(start.pointer.position, computed, transform);
    commands
        .entity(node)
        .insert(Grip(bearing(at) - face.turn(shown.0)));
    cursor.0 = Some(SystemCursorIcon::Grabbing);
}

fn let_go(
    mut end: On<PointerDragEnd>,
    mut cursor: ResMut<OverrideCursor>,
) {
    if end.button != PointerButton::Primary {
        return;
    }
    end.propagate(false);
    cursor.0 = None;
}

fn dragged(
    mut drag: On<PointerDrag>,
    rings: Query<(&Face, &Grip, &ComputedNode, &UiGlobalTransform)>,
    mut commands: Commands,
) {
    if drag.button != PointerButton::Primary {
        return;
    }
    drag.propagate(false);
    let node = drag.event_target();
    let Ok((face, grip, computed, transform)) = rings.get(node)
    else {
        return;
    };
    // Measured from the grip alone, never from what the dial shows,
    // which follows the world a frame behind.
    let at = from_middle(drag.pointer.position, computed, transform);
    let amount = amount_at(face, bearing(at) - grip.0);
    commands.queue(move |world: &mut World| {
        let Some(handler) = world
            .get_mut::<Turns>(node)
            .and_then(|mut turns| turns.0.take())
        else {
            return;
        };
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
    fn a_grip_turns_the_knob_with_the_pointer_and_wraps_at_the_foot()
    {
        let face = Face::HOURS;
        // Taken hold of at the top while the dial shows 18:00.
        let grip = bearing(Vec2::new(0.0, -1.0)) - face.turn(18.0);
        // A quarter turn clockwise later it shows midnight.
        let quarter = bearing(Vec2::new(1.0, 0.0)) - grip;
        assert!(amount_at(&face, quarter).abs() < 1e-3);
        // Either side of the foot, where the bearing wraps, the hours
        // stay close.
        let left =
            amount_at(&face, bearing(Vec2::new(-0.1, 1.0)) - grip);
        let right =
            amount_at(&face, bearing(Vec2::new(0.1, 1.0)) - grip);
        let apart = (left - right).rem_euclid(24.0);
        assert!(apart.min(24.0 - apart) < 1.0);
        // Noon on the rim is straight above the middle.
        let top = place(Face::HOURS.turn(12.0), SIDE, SIDE / 2.0);
        assert!((top.x - SIDE / 2.0).abs() < 1e-3);
        assert!(top.y.abs() < 1e-3);
    }
}
