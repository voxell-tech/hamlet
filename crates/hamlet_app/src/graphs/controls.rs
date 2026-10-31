//! The controls on a card's unwired input rows: a field for a number,
//! a whole number or each axis of a point or vector, a switch, a
//! colour's four fields with a swatch, a choice that opens its list
//! in the menu layer, and a mark that puts a fixed input back to its
//! default. Changing a value fixes the input; until then it shows its
//! default. A group's title is a field on its title bar.

use bevy::color::Color;
use bevy::ecs::world::World;
use bevy::math::Vec3;
use bevy::ui::{AlignItems, Display, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, frame, icon_button,
    label, number_field, row, text_field,
};
use bevy_fynix::{ScopedExt, Signal, ViewExt, resource};
use hamlet_blueprint::{
    Constant, Edit, Listed, NodeId, Setting, ValueType,
};
use hamlet_ui::styles::{note, text_input};
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::switch::switch;
use hamlet_ui::{AnyView, with_cx};

use super::Editor;
use super::canvas::Input;
use super::flavor::Flavor;

/// The width of a number's field, and of each axis of a point's.
const NUMBER: f32 = 58.0;
const AXIS: f32 = 38.0;
/// The width of each of a colour's four fields.
const CHANNEL: f32 = 32.0;
const MARK: f32 = 12.0;

/// How an input of `node` is set now.
pub(super) fn current<F: Flavor>(
    editor: &Editor<F>,
    node: NodeId,
    input: &str,
) -> Option<Setting> {
    editor
        .sheet
        .graph
        .node(node)
        .and_then(|found| found.settings.get(input).cloned())
}

/// The type an input's value is written in, none for one that can
/// only be wired. A field is set as a number, the same everywhere. A
/// vector or colour with nothing to start from is only wired:
/// unwired, it is the surface's own value.
pub(super) fn written(input: &Input) -> Option<ValueType> {
    match input.value_type {
        ValueType::Field | ValueType::Number => {
            Some(ValueType::Number)
        }
        ValueType::Integer => Some(ValueType::Integer),
        ValueType::Bool => Some(ValueType::Bool),
        ValueType::Point => Some(ValueType::Point),
        ValueType::Choice => Some(ValueType::Choice),
        ValueType::Vector2
        | ValueType::Vector3
        | ValueType::Color => {
            input.initial.as_ref().map(|_| input.value_type)
        }
        _ => None,
    }
}

/// The value an input shows: what it is fixed to, else its default.
pub(super) fn value_of<F: Flavor>(
    editor: &Editor<F>,
    node: NodeId,
    input: &Input,
) -> Option<Constant> {
    match current(editor, node, &input.name) {
        Some(Setting::Fixed(constant)) => Some(constant),
        _ => default_of(input),
    }
}

/// What an input is when nothing sets it.
fn default_of(input: &Input) -> Option<Constant> {
    if let Some(initial) = &input.initial {
        return Some(initial.clone());
    }
    Some(match written(input)? {
        ValueType::Bool => Constant::Bool(true),
        ValueType::Point => Constant::Point(Vec3::ZERO),
        ValueType::Integer => Constant::Integer(0),
        ValueType::Choice => Constant::Choice(
            input
                .choices
                .first()
                .copied()
                .unwrap_or_default()
                .to_owned(),
        ),
        _ => Constant::Number(0.0),
    })
}

pub(super) fn put<F: Flavor>(
    world: &mut World,
    node: NodeId,
    input: &str,
    setting: Option<Setting>,
) {
    world.resource_mut::<Editor<F>>().apply(Edit::Set {
        node,
        input: input.to_owned(),
        setting,
    });
}

/// The controls for an unwired input of `node`, as it is set when the
/// card is built, or nothing for one that can only be wired.
pub(super) fn control<F: Flavor>(
    node: NodeId,
    input: Input,
    setting: Option<&Setting>,
) -> Option<AnyView> {
    let value = match setting {
        Some(Setting::FromParent(name)) => {
            return Some(pill(name.replace('_', " ")));
        }
        Some(Setting::Random(_)) => {
            return Some(pill("random".into()));
        }
        _ => match written(&input)? {
            ValueType::Number | ValueType::Integer => {
                number::<F>(node, input.clone(), None)
            }
            ValueType::Point | ValueType::Vector3 => {
                axes::<F>(node, &input, 3)
            }
            ValueType::Vector2 => axes::<F>(node, &input, 2),
            ValueType::Color => row((
                axes::<F>(node, &input, 4),
                swatch::<F>(node, input.clone()),
            ))
            .gap(2.0)
            .align(AlignItems::Center)
            .boxed(),
            ValueType::Bool => flag::<F>(node, input.clone()),
            _ if !input.choices.is_empty()
                || input.listed.is_some() =>
            {
                choice::<F>(node, input.clone())
            }
            _ => words::<F>(node, input.clone()),
        },
    };
    Some(
        row((value, reset::<F>(node, input.name)))
            .gap(2.0)
            .align(AlignItems::Center)
            .boxed(),
    )
}

/// What sets an input other than a value of its own, in place of its
/// controls.
fn pill(text: String) -> AnyView {
    label(text).wrap(false).tone(Tone::Accent).boxed()
}

/// A row of fields, one for each of the first `count` axes of a
/// point, a vector or a colour.
fn axes<F: Flavor>(
    node: NodeId,
    input: &Input,
    count: usize,
) -> AnyView {
    row((0..count)
        .map(|axis| number::<F>(node, input.clone(), Some(axis)))
        .collect::<Vec<_>>())
    .gap(2.0)
    .boxed()
}

/// A field for a number, whole or not, or for one axis of a point, a
/// vector or a colour.
fn number<F: Flavor>(
    node: NodeId,
    input: Input,
    axis: Option<usize>,
) -> AnyView {
    let read = input.clone();
    let whole = input.value_type == ValueType::Integer;
    let colour = input.value_type == ValueType::Color;
    let field = number_field(
        resource::<Editor<F>, _>(move |editor| {
            match (value_of(editor, node, &read), axis) {
                (Some(Constant::Number(n)), None) => f64::from(n),
                (Some(Constant::Integer(n)), None) => f64::from(n),
                (Some(Constant::Point(p)), Some(axis)) => {
                    f64::from(p[axis])
                }
                (Some(Constant::Color(c)), Some(axis)) => {
                    f64::from(c[axis])
                }
                _ => 0.0,
            }
        }),
        move |world: &mut World, n: f64| {
            let n = n as f32;
            let constant = match axis {
                None if whole => Constant::Integer(n.round() as i32),
                None => Constant::Number(n),
                Some(axis) => {
                    match value_of(
                        world.resource::<Editor<F>>(),
                        node,
                        &input,
                    ) {
                        Some(Constant::Color(mut c)) => {
                            c[axis] = n;
                            Constant::Color(c)
                        }
                        Some(Constant::Point(mut p)) => {
                            p[axis] = n;
                            Constant::Point(p)
                        }
                        _ => {
                            let mut p = Vec3::ZERO;
                            p[axis] = n;
                            Constant::Point(p)
                        }
                    }
                }
            };
            put::<F>(
                world,
                node,
                &input.name,
                Some(Setting::Fixed(constant)),
            );
        },
    )
    .step(if whole { 1.0 } else { 0.05 })
    .precision(if whole { 0 } else { 2 });
    let field = if colour { field.range(0.0, 1.0) } else { field };
    let width = match (axis, colour) {
        (None, _) => NUMBER,
        (Some(_), false) => AXIS,
        (Some(_), true) => CHANNEL,
    };
    field.width(px(width)).rules(text_input).boxed()
}

/// A small square of the colour an input is set to.
fn swatch<F: Flavor>(node: NodeId, input: Input) -> AnyView {
    let shown =
        resource::<Editor<F>, _>(move |editor| {
            match value_of(editor, node, &input) {
                Some(Constant::Color(c)) => {
                    Color::srgba(c.x, c.y, c.z, c.w)
                }
                _ => Color::NONE,
            }
        });
    with_cx(move |cx| {
        frame()
            .width(px(MARK))
            .height(px(MARK))
            .radius(2.0)
            .border(1.0)
            .border_color(cx.theme().palette.border)
            .fill(shown)
    })
    .boxed()
}

fn flag<F: Flavor>(node: NodeId, input: Input) -> AnyView {
    let read = input.clone();
    switch(
        resource::<Editor<F>, _>(move |editor| {
            matches!(
                value_of(editor, node, &read),
                Some(Constant::Bool(true))
            )
        }),
        move |world, on| {
            put::<F>(
                world,
                node,
                &input.name,
                Some(Setting::Fixed(Constant::Bool(on))),
            );
        },
    )
    .boxed()
}

/// A choice: its value and a chevron, which open the list of choices
/// at the pointer.
fn choice<F: Flavor>(node: NodeId, input: Input) -> AnyView {
    let read = input.clone();
    button(
        row((
            label(resource::<Editor<F>, _>(move |editor| {
                match value_of(editor, node, &read) {
                    Some(Constant::Choice(choice)) => {
                        choice.replace('_', " ")
                    }
                    _ => String::new(),
                }
            }))
            .wrap(false),
            glyph(Glyph::Chevron, MARK).tone(Tone::Dim),
        ))
        .gap(4.0)
        .align(AlignItems::Center),
    )
    .padding(bevy::ui::UiRect::axes(px(6.0), px(0.0)))
    .rules(text_input)
    .on_activate(move |world| {
        let catalogue = &world.resource::<Editor<F>>().rules.assets;
        let choices = match input.listed {
            Some(Listed::Assets) => catalogue.names(),
            Some(Listed::Sockets) => catalogue.socket_names(),
            None => input
                .choices
                .iter()
                .map(|c| (*c).to_owned())
                .collect(),
        };
        super::menu::choose::<F>(
            world,
            node,
            input.name.clone(),
            choices,
        );
    })
    .boxed()
}

/// Any text, for a choice with no list.
fn words<F: Flavor>(node: NodeId, input: Input) -> AnyView {
    let read = input.clone();
    text_field(
        resource::<Editor<F>, _>(move |editor| {
            match value_of(editor, node, &read) {
                Some(Constant::Choice(text)) => text,
                _ => String::new(),
            }
        }),
        move |world, text| {
            put::<F>(
                world,
                node,
                &input.name,
                Some(Setting::Fixed(Constant::Choice(text))),
            );
        },
    )
    .width(px(NUMBER * 1.5))
    .rules(text_input)
    .boxed()
}

/// The field a group's title is written in.
pub(super) fn title<F: Flavor>(at: usize) -> AnyView {
    text_field(
        resource::<Editor<F>, _>(move |editor| {
            editor
                .sheet
                .groups
                .get(at)
                .map(|group| group.title.clone())
                .unwrap_or_default()
        }),
        move |world, text| {
            world.resource_mut::<Editor<F>>().retitle(at, text);
        },
    )
    .width(px(NUMBER * 3.0))
    .rules(text_input)
    .boxed()
}

/// The mark that puts an input back to its default, shown while it is
/// fixed.
fn reset<F: Flavor>(node: NodeId, input: String) -> AnyView {
    let shown: Signal<Display> = {
        let input = input.clone();
        resource::<Editor<F>, _>(move |editor| {
            match current(editor, node, &input) {
                Some(Setting::Fixed(_)) => Display::Flex,
                _ => Display::None,
            }
        })
    };
    button(glyph(Glyph::Restore, MARK))
        .display(shown)
        .rules(icon_button)
        .tooltip(|| note("Back to the default"))
        .on_activate(move |world| put::<F>(world, node, &input, None))
        .boxed()
}
