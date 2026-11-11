//! The colour control the editors share, as Blender's: a bar of the
//! colour that opens a picker over everything. The picker has a wheel
//! of hue round it and saturation out from its middle beside a bar of
//! value, then the colour's RGB or HSV fields, each a slider and its
//! number, and its hex code. A graph's parameters, a material's
//! values and anything else that holds a colour use it, each reading
//! and writing its colour as linear RGBA, as shaders read it; RGB
//! shows that, while HSV, the wheel and the hex code show how it
//! looks.

use std::sync::Arc;

use bevy::app::{App, Update};
use bevy::color::{Alpha, Color, Hsva, LinearRgba, Srgba};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::event::EntityEvent;
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Query};
use bevy::ecs::world::World;
use bevy::math::{Rect, Vec2, Vec4};
use bevy::picking::events::{PointerDrag, PointerPress};
use bevy::picking::pointer::PointerButton;
use bevy::ui::{
    AlignItems, AngularColorStop, BackgroundGradient, ColorStop,
    ComputedNode, ConicGradient, InterpolationColorSpace,
    JustifyContent, LinearGradient, PositionType, RadialGradient,
    RadialGradientShape, UiGlobalTransform, UiPosition, UiRect, Val,
    percent, px,
};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, column, frame,
    label, number_field, overlay, popup, row, slider, text_field,
};
use bevy_fynix::{ScopedExt, ViewExt, keyed, resource};
use hamlet_ui::styles::{note, segmented_words, text_input};
use hamlet_ui::{AnyView, with_cx};

/// How tall a colour's bar is, which opens its picker.
const BAR_HEIGHT: f32 = 20.0;
/// The wheel's side.
const WHEEL: f32 = 150.0;
/// The value bar's width.
const VALUE: f32 = 16.0;
/// The side of the ring that marks the colour on the wheel.
const MARK: f32 = 12.0;
/// How wide the picker is.
const WIDE: f32 = 236.0;
/// How wide a field's number is beside its slider.
const NUMBER: f32 = 64.0;

/// Reads the colour of the control whose picker is open.
type Reader = Arc<dyn Fn(&World) -> Option<Vec4> + Send + Sync>;
/// Writes the colour of the control whose picker is open.
type Writer = Arc<dyn Fn(&mut World, Vec4) + Send + Sync>;

/// Which fields the picker shows under the wheel.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Mode {
    /// Red, green and blue, linear, as shaders read them.
    Rgb,
    /// Hue, saturation and value of how it looks.
    #[default]
    Hsv,
}

/// The colour whose picker is open, by the id its control was given,
/// where its bar is, how to read and write it, and the hue,
/// saturation and value it shows: kept while it is open, so a grey or
/// a black keeps the hue it was given.
#[derive(Resource)]
pub struct Picking {
    id: Option<String>,
    anchor: Rect,
    read: Option<Reader>,
    write: Option<Writer>,
    hsva: Hsva,
    mode: Mode,
}

impl Default for Picking {
    fn default() -> Self {
        Self {
            id: None,
            anchor: Rect::default(),
            read: None,
            write: None,
            hsva: Hsva::new(0.0, 0.0, 0.5, 1.0),
            mode: Mode::default(),
        }
    }
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Picking>()
        .add_observer(pressed)
        .add_observer(dragged)
        .add_systems(Update, follow);
}

/// A linear colour as the picker shows it: hue, saturation and value
/// of how it looks, as Blender's perceptual picker.
fn hsva_of(linear: Vec4) -> Hsva {
    Hsva::from(srgb_of(linear))
}

fn srgb_of(linear: Vec4) -> Srgba {
    Srgba::from(LinearRgba::new(
        linear.x, linear.y, linear.z, linear.w,
    ))
}

/// What the picker shows, as the linear colour shaders read.
fn linear_of(colour: impl Into<LinearRgba>) -> Vec4 {
    let linear: LinearRgba = colour.into();
    Vec4::new(linear.red, linear.green, linear.blue, linear.alpha)
}

/// `next`, keeping the hue of `was` where `next` has none, being a
/// grey, and its saturation where it has none either, being black.
fn kept(was: Hsva, mut next: Hsva) -> Hsva {
    if next.value <= 1e-4 {
        next.saturation = was.saturation;
    }
    if next.saturation <= 1e-4 || next.value <= 1e-4 {
        next.hue = was.hue;
    }
    next
}

/// The colour as a hex code of how it looks, as Blender gives it:
/// gamma corrected, with no alpha.
fn hex_of(hsva: Hsva) -> String {
    let srgb = Srgba::from(hsva);
    let byte =
        |channel: f32| (channel.clamp(0.0, 1.0) * 255.0).round();
    format!(
        "#{:02X}{:02X}{:02X}",
        byte(srgb.red) as u8,
        byte(srgb.green) as u8,
        byte(srgb.blue) as u8
    )
}

/// The control of a colour that `read` finds in the resource `R`,
/// none while there is none, and `write` sets: a bar of the colour,
/// which opens the picker under it. `id` tells it from every other
/// colour control shown, so one picker is open at a time.
pub fn colour<R, F, W>(id: String, read: F, write: W) -> AnyView
where
    R: Resource,
    F: Fn(&R) -> Option<Vec4> + Clone + Send + Sync + 'static,
    W: Fn(&mut World, Vec4) + Clone + Send + Sync + 'static,
{
    let reader: Reader = {
        let read = read.clone();
        Arc::new(move |world: &World| {
            world.get_resource::<R>().and_then(&read)
        })
    };
    let writer: Writer = Arc::new(write);
    with_cx(move |cx| {
        let border = cx.theme().palette.border;
        let (id, reader, writer) =
            (id.clone(), reader.clone(), writer.clone());
        let read = read.clone();
        let shown = resource::<R, _>(move |held| {
            // Linear, as shaders read it, and opaque, as Blender
            // shows it.
            read(held).map_or(Color::NONE, |c| {
                Color::linear_rgba(c.x, c.y, c.z, 1.0)
            })
        });
        button(
            frame()
                .width(percent(100.0))
                .height(px(BAR_HEIGHT))
                .radius(3.0)
                .border(1.0)
                .border_color(border)
                .fill(shown),
        )
        .width(percent(100.0))
        .min_width(px(0.0))
        .tooltip(|| note("Pick the colour"))
        .on_activate_with(move |world, node| {
            open(world, &id, node, reader.clone(), writer.clone());
        })
    })
    .boxed()
}

/// Opens the picker of the colour `id`, under its bar `node`, or
/// closes it when it is open.
fn open(
    world: &mut World,
    id: &str,
    node: Entity,
    read: Reader,
    write: Writer,
) {
    if world.resource::<Picking>().id.as_deref() == Some(id) {
        close(world);
        return;
    }
    let anchor = rect_of(world, node).unwrap_or_default();
    let now = read(world);
    let mut picking = world.resource_mut::<Picking>();
    picking.id = Some(id.to_owned());
    picking.anchor = anchor;
    picking.read = Some(read);
    picking.write = Some(write);
    if let Some(linear) = now {
        let was = picking.hsva;
        picking.hsva = kept(was, hsva_of(linear));
    }
}

fn close(world: &mut World) {
    let mut picking = world.resource_mut::<Picking>();
    picking.id = None;
    picking.read = None;
    picking.write = None;
}

/// The rect of `node` on the window, in logical pixels.
fn rect_of(world: &World, node: Entity) -> Option<Rect> {
    let computed = world.get::<ComputedNode>(node)?;
    let placed = world.get::<UiGlobalTransform>(node)?;
    let scale = computed.inverse_scale_factor();
    let (_, _, centre) = placed.to_scale_angle_translation();
    Some(Rect::from_center_size(
        centre * scale,
        computed.size() * scale,
    ))
}

/// Keeps what the open picker shows up to date with its colour when
/// something else changes it, as an undo.
fn follow(world: &mut World) {
    let Some(read) = world.resource::<Picking>().read.clone() else {
        return;
    };
    let Some(now) = read(world) else {
        return;
    };
    let shown = linear_of(world.resource::<Picking>().hsva);
    if (now - shown).abs().max_element() > 1e-3 {
        let mut picking = world.resource_mut::<Picking>();
        let was = picking.hsva;
        picking.hsva = kept(was, hsva_of(now));
    }
}

/// Sets the colour the open picker shows to what `change` makes of
/// it, and writes it.
fn change(world: &mut World, change: impl FnOnce(&mut Hsva)) {
    let (hsva, write) = {
        let mut picking = world.resource_mut::<Picking>();
        change(&mut picking.hsva);
        (picking.hsva, picking.write.clone())
    };
    if let Some(write) = write {
        write(world, linear_of(hsva));
    }
}

/// The open picker, over everything, under its colour's bar.
pub fn popups() -> AnyView {
    keyed(
        resource::<Picking, _>(|picking| {
            picking.id.as_ref().map(|_| picking.anchor)
        }),
        |anchor: &Option<Rect>| {
            let Some(anchor) = *anchor else {
                return frame().boxed();
            };
            popup(anchor, (picker(),))
                .on_dismiss(|world, _| close(world))
                .width(px(WIDE))
                .boxed()
        },
    )
    .within(overlay(()))
    .boxed()
}

/// The picker: the wheel and the value bar, the choice of RGB or HSV,
/// their fields, and the hex code.
fn picker() -> AnyView {
    with_cx(|cx| {
        let gap = cx.theme().space.gap;
        let modes = keyed(
            resource::<Picking, _>(|picking| picking.mode),
            |mode: &Mode| {
                let picked = match mode {
                    Mode::Rgb => 0,
                    Mode::Hsv => 1,
                };
                let choice = row((segmented_words(
                    ["RGB", "HSV"],
                    picked,
                    |world, at| {
                        world.resource_mut::<Picking>().mode =
                            if at == 0 {
                                Mode::Rgb
                            } else {
                                Mode::Hsv
                            };
                    },
                ),))
                .width(percent(100.0))
                .justify(JustifyContent::Center);
                let fields = match mode {
                    Mode::Rgb => &RGB,
                    Mode::Hsv => &HSV,
                };
                column(
                    std::iter::once(choice.boxed())
                        .chain(fields.iter().map(field))
                        .collect::<Vec<_>>(),
                )
                .gap(4.0)
                .width(percent(100.0))
                .boxed()
            },
        )
        .within(column(()).gap(0.0).width(percent(100.0)));
        column((
            row((wheel(), value_bar()))
                .gap(10.0)
                .width(percent(100.0))
                .justify(JustifyContent::Center)
                .align(AlignItems::Center)
                .padding(UiRect::vertical(px(4.0))),
            modes,
            hex(),
        ))
        .gap(gap / 2.0)
        .width(percent(100.0))
    })
    .boxed()
}

/// One field of the picker: what it is called, how it reads the
/// colour shown and how it changes it.
struct Field {
    label: &'static str,
    read: fn(Hsva) -> f32,
    write: fn(&mut Hsva, f32),
}

/// Sets the linear channel `at` of `hsva` to `to`.
fn set_linear(hsva: &mut Hsva, at: usize, to: f32) {
    let mut linear = linear_of(*hsva);
    linear[at] = to.max(0.0);
    *hsva = kept(*hsva, hsva_of(linear));
}

/// Red, green, blue and alpha, linear, as Blender's RGB.
const RGB: [Field; 4] = [
    Field {
        label: "R",
        read: |hsva| linear_of(hsva).x,
        write: |hsva, to| set_linear(hsva, 0, to),
    },
    Field {
        label: "G",
        read: |hsva| linear_of(hsva).y,
        write: |hsva, to| set_linear(hsva, 1, to),
    },
    Field {
        label: "B",
        read: |hsva| linear_of(hsva).z,
        write: |hsva, to| set_linear(hsva, 2, to),
    },
    Field {
        label: "A",
        read: |hsva| hsva.alpha,
        write: |hsva, to| hsva.alpha = to.clamp(0.0, 1.0),
    },
];

/// Hue, saturation, value and alpha, each from 0 to 1, as Blender's
/// HSV.
const HSV: [Field; 4] = [
    Field {
        label: "H",
        read: |hsva| hsva.hue / 360.0,
        write: |hsva, to| hsva.hue = to.clamp(0.0, 1.0) * 360.0,
    },
    Field {
        label: "S",
        read: |hsva| hsva.saturation,
        write: |hsva, to| hsva.saturation = to.clamp(0.0, 1.0),
    },
    Field {
        label: "V",
        read: |hsva| hsva.value,
        write: |hsva, to| hsva.value = to.clamp(0.0, 1.0),
    },
    Field {
        label: "A",
        read: |hsva| hsva.alpha,
        write: |hsva, to| hsva.alpha = to.clamp(0.0, 1.0),
    },
];

/// A field's row: its letter, a slider from 0 to 1, and its number to
/// three places, which may be typed.
fn field(field: &Field) -> AnyView {
    let Field {
        label: name,
        read,
        write,
    } = *field;
    row((
        label(name).wrap(false),
        slider(resource::<Picking, _>(move |picking| {
            read(picking.hsva)
        }))
        .on_change(move |world, to| {
            change(world, |hsva| write(hsva, to));
        })
        .width(percent(100.0))
        .min_width(px(0.0)),
        row((number_field(
            resource::<Picking, _>(move |picking| {
                f64::from(read(picking.hsva))
            }),
            move |world: &mut World, to: f64| {
                change(world, |hsva| write(hsva, to as f32));
            },
        )
        .step(0.01)
        .precision(3)
        .width(percent(100.0))
        .min_width(px(0.0))
        .rules(text_input),))
        .width(px(NUMBER))
        .shrink(0.0),
    ))
    .gap(6.0)
    .align(AlignItems::Center)
    .width(percent(100.0))
    .boxed()
}

/// The hex code of how the colour looks, which a code typed or pasted
/// sets.
fn hex() -> AnyView {
    row((
        label("Hex").wrap(false),
        text_field(
            resource::<Picking, _>(|picking| hex_of(picking.hsva)),
            |world, text| {
                let Ok(srgb) = Srgba::hex(text.trim()) else {
                    return;
                };
                change(world, |hsva| {
                    let alpha = hsva.alpha;
                    *hsva = kept(
                        *hsva,
                        Hsva::from(srgb.with_alpha(alpha)),
                    );
                });
            },
        )
        .width(percent(100.0))
        .min_width(px(0.0))
        .rules(text_input),
    ))
    .gap(6.0)
    .align(AlignItems::Center)
    .width(percent(100.0))
    .tooltip(|| note("Gamma corrected"))
    .boxed()
}

/// What a press or a drag on a part of the picker sets.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Sets {
    /// Hue round the wheel and saturation out from its middle.
    HueSaturation,
    /// Value, from the bar's foot to its top.
    Value,
}

/// On the wheel and the value bar: what they set.
#[derive(Component)]
struct Pad(Sets);

/// How far round the wheel, in degrees clockwise from its top, the
/// hue `hue` is: as Blender's, cyan at the top and red at the foot.
/// The same turn takes a place round the wheel back to its hue.
fn round_of(hue: f32) -> f32 {
    (hue + 180.0).rem_euclid(360.0)
}

/// Where the colour `hsva` is on a wheel `side` wide, from its top
/// left.
fn on_wheel(hsva: Hsva, side: f32) -> Vec2 {
    let turn = round_of(hsva.hue).to_radians();
    let out = hsva.saturation.clamp(0.0, 1.0) * side / 2.0;
    Vec2::new(
        side / 2.0 + out * turn.sin(),
        side / 2.0 - out * turn.cos(),
    )
}

/// The hue and saturation at `at`, from a wheel's top left, on a
/// wheel `side` wide; no hue at its very middle, where every hue is
/// white.
fn off_wheel(at: Vec2, side: f32) -> (Option<f32>, f32) {
    let from = at - Vec2::splat(side / 2.0);
    let saturation = (from.length() / (side / 2.0)).min(1.0);
    let hue = (from.length() > 0.5)
        .then(|| round_of(from.x.atan2(-from.y).to_degrees()));
    (hue, saturation)
}

fn pressed(
    mut press: On<PointerPress>,
    pads: Query<(&Pad, &ComputedNode, &UiGlobalTransform)>,
    mut commands: Commands,
) {
    if press.button != PointerButton::Primary
        || !pads.contains(press.event_target())
    {
        return;
    }
    press.propagate(false);
    let at = press.pointer.position;
    aim(press.event_target(), at, &pads, &mut commands);
}

fn dragged(
    mut drag: On<PointerDrag>,
    pads: Query<(&Pad, &ComputedNode, &UiGlobalTransform)>,
    mut commands: Commands,
) {
    if drag.button != PointerButton::Primary
        || !pads.contains(drag.event_target())
    {
        return;
    }
    drag.propagate(false);
    let at = drag.pointer.position;
    aim(drag.event_target(), at, &pads, &mut commands);
}

/// Sets what the pad `node` sets to what is under a pointer at `at`,
/// in the window's logical pixels, and writes the colour.
fn aim(
    node: Entity,
    at: Vec2,
    pads: &Query<(&Pad, &ComputedNode, &UiGlobalTransform)>,
    commands: &mut Commands,
) {
    let Ok((Pad(sets), computed, placed)) = pads.get(node) else {
        return;
    };
    let scale = computed.inverse_scale_factor();
    let (_, _, centre) = placed.to_scale_angle_translation();
    let size = computed.size() * scale;
    let local = at - (centre * scale - size / 2.0);
    let sets = *sets;
    commands.queue(move |world: &mut World| {
        change(world, |hsva| match sets {
            Sets::HueSaturation => {
                let (hue, saturation) = off_wheel(local, size.x);
                if let Some(hue) = hue {
                    hsva.hue = hue;
                }
                hsva.saturation = saturation;
            }
            Sets::Value => {
                let down =
                    (local.y / size.y.max(1.0)).clamp(0.0, 1.0);
                hsva.value = 1.0 - down;
            }
        });
    });
}

/// The wheel: every hue round it, white at its middle, as dark as the
/// colour's value, with a ring where the colour is.
fn wheel() -> AnyView {
    let round = WHEEL / 2.0;
    // Clockwise from the top, from cyan back to cyan.
    let hues = (0..=6)
        .map(|step| {
            AngularColorStop::auto(Hsva::new(
                (180.0 + step as f32 * 60.0).rem_euclid(360.0),
                1.0,
                1.0,
                1.0,
            ))
        })
        .collect();
    let hue = BackgroundGradient(vec![
        ConicGradient::new(UiPosition::CENTER, hues)
            .in_color_space(InterpolationColorSpace::Srgba)
            .into(),
    ]);
    let white = BackgroundGradient(vec![
        RadialGradient::new(
            UiPosition::CENTER,
            RadialGradientShape::ClosestSide,
            vec![
                ColorStop::auto(Color::WHITE),
                ColorStop::auto(Color::WHITE.with_alpha(0.0)),
            ],
        )
        .in_color_space(InterpolationColorSpace::Srgba)
        .into(),
    ]);
    let dark = resource::<Picking, _>(|picking| {
        Color::BLACK.with_alpha(1.0 - picking.hsva.value)
    });
    let mark = frame()
        .position(PositionType::Absolute)
        .width(px(MARK))
        .height(px(MARK))
        .radius(MARK / 2.0)
        .border(2.0)
        .border_color(Color::WHITE)
        .inset(resource::<Picking, _>(|picking| {
            let at = on_wheel(picking.hsva, WHEEL) - MARK / 2.0;
            UiRect {
                left: px(at.x),
                top: px(at.y),
                right: Val::Auto,
                bottom: Val::Auto,
            }
        }));
    column((
        overlay(()).radius(round).with(white),
        overlay(()).radius(round).fill(dark),
        mark,
    ))
    .width(px(WHEEL))
    .height(px(WHEEL))
    .shrink(0.0)
    .radius(round)
    .with(hue)
    .with(Pad(Sets::HueSaturation))
    .boxed()
}

/// The bar of value: the colour at its brightest at the top, black at
/// the foot, with a line at the colour's value.
fn value_bar() -> AnyView {
    let brightest = resource::<Picking, _>(|picking| {
        Color::from(Hsva {
            value: 1.0,
            alpha: 1.0,
            ..picking.hsva
        })
    });
    let darker = BackgroundGradient(vec![
        LinearGradient::to_bottom(vec![
            ColorStop::auto(Color::BLACK.with_alpha(0.0)),
            ColorStop::auto(Color::BLACK),
        ])
        .in_color_space(InterpolationColorSpace::Srgba)
        .into(),
    ]);
    let line = frame()
        .position(PositionType::Absolute)
        .width(px(VALUE + 4.0))
        .height(px(4.0))
        .radius(2.0)
        .border(1.0)
        .border_color(Color::WHITE)
        .inset(resource::<Picking, _>(|picking| UiRect {
            left: px(-2.0),
            top: px((1.0 - picking.hsva.value) * WHEEL - 2.0),
            right: Val::Auto,
            bottom: Val::Auto,
        }));
    column((overlay(()).radius(3.0).with(darker), line))
        .width(px(VALUE))
        .height(px(WHEEL))
        .shrink(0.0)
        .radius(3.0)
        .fill(brightest)
        .with(Pad(Sets::Value))
        .boxed()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_colour_put_on_the_wheel_is_read_back_from_where_it_is() {
        for (hue, saturation) in
            [(0.0, 1.0), (90.0, 0.5), (215.0, 0.8)]
        {
            let hsva = Hsva::new(hue, saturation, 0.7, 1.0);
            let at = on_wheel(hsva, WHEEL);
            let (read, out) = off_wheel(at, WHEEL);
            let off = (read.unwrap() - hue).rem_euclid(360.0);
            assert!(off < 0.01 || off > 359.99, "{hue}");
            assert!((out - saturation).abs() < 0.001, "{saturation}");
        }
    }

    #[test]
    fn as_in_blender_cyan_is_at_the_top_and_red_at_the_foot() {
        let at = |hue| on_wheel(Hsva::new(hue, 1.0, 1.0, 1.0), WHEEL);
        assert!(
            at(180.0).distance(Vec2::new(WHEEL / 2.0, 0.0)) < 0.01
        );
        assert!(
            at(0.0).distance(Vec2::new(WHEEL / 2.0, WHEEL)) < 0.01
        );
        // Green to the upper left, blue to the upper right.
        assert!(
            at(120.0).x < WHEEL / 2.0 && at(120.0).y < WHEEL / 2.0
        );
        assert!(
            at(240.0).x > WHEEL / 2.0 && at(240.0).y < WHEEL / 2.0
        );
        assert_eq!(
            off_wheel(Vec2::splat(WHEEL / 2.0), WHEEL),
            (None, 0.0)
        );
    }

    #[test]
    fn the_hex_code_is_gamma_corrected_without_alpha() {
        let grey = hsva_of(linear_of(Srgba::new(0.8, 0.8, 0.8, 0.5)));
        assert_eq!(hex_of(grey), "#CCCCCC");
    }

    #[test]
    fn a_grey_keeps_the_hue_it_was_given() {
        let was = Hsva::new(200.0, 0.6, 0.5, 1.0);
        let grey = Hsva::new(0.0, 0.0, 0.5, 1.0);
        assert_eq!(kept(was, grey).hue, 200.0);
    }
}
