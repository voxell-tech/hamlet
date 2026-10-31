//! The controls on a card's unwired input rows: a field for a number
//! or each axis of a point, a switch, a choice that opens its list in
//! the menu layer, and a mark that puts a fixed input back to its
//! default. Changing a value fixes the input; until then it shows its
//! default. A group's title is a field on its title bar.

use bevy::ecs::world::World;
use bevy::math::Vec3;
use bevy::ui::{AlignItems, Display, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, icon_button, label,
    number_field, row, text_field,
};
use bevy_fynix::{ScopedExt, Signal, ViewExt, resource};
use hamlet_blueprint::{Constant, Edit, NodeId, Setting, ValueType};
use hamlet_ui::AnyView;
use hamlet_ui::styles::{note, text_input};
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::switch::switch;

use super::Editor;
use super::canvas::Input;

/// The width of a number's field, and of each axis of a point's.
const NUMBER: f32 = 58.0;
const AXIS: f32 = 38.0;
const MARK: f32 = 12.0;

/// How an input of `node` is set now.
pub(super) fn current(
    editor: &Editor,
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
/// only be wired. A field is set as a number, the same everywhere.
pub(super) fn written(value_type: ValueType) -> Option<ValueType> {
    match value_type {
        ValueType::Field | ValueType::Number => {
            Some(ValueType::Number)
        }
        ValueType::Bool => Some(ValueType::Bool),
        ValueType::Point => Some(ValueType::Point),
        ValueType::Choice => Some(ValueType::Choice),
        _ => None,
    }
}

/// The value an input shows: what it is fixed to, else its default.
pub(super) fn value_of(
    editor: &Editor,
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
    Some(match written(input.value_type)? {
        ValueType::Bool => Constant::Bool(true),
        ValueType::Point => Constant::Point(Vec3::ZERO),
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

pub(super) fn put(
    world: &mut World,
    node: NodeId,
    input: &str,
    setting: Option<Setting>,
) {
    world.resource_mut::<Editor>().apply(Edit::Set {
        node,
        input: input.to_owned(),
        setting,
    });
}

/// The controls for an unwired input of `node`, as it is set when the
/// card is built, or nothing for one that can only be wired.
pub(super) fn control(
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
        _ => match written(input.value_type)? {
            ValueType::Number => number(node, input.clone(), None),
            ValueType::Point => row((
                number(node, input.clone(), Some(0)),
                number(node, input.clone(), Some(1)),
                number(node, input.clone(), Some(2)),
            ))
            .gap(2.0)
            .boxed(),
            ValueType::Bool => flag(node, input.clone()),
            _ if !input.choices.is_empty() || input.assets => {
                choice(node, input.clone())
            }
            _ => words(node, input.clone()),
        },
    };
    Some(
        row((value, reset(node, input.name)))
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

/// A field for a number, or for one axis of a point.
fn number(
    node: NodeId,
    input: Input,
    axis: Option<usize>,
) -> AnyView {
    let read = input.clone();
    number_field(
        resource::<Editor, _>(move |editor| {
            match (value_of(editor, node, &read), axis) {
                (Some(Constant::Number(n)), None) => f64::from(n),
                (Some(Constant::Point(p)), Some(axis)) => {
                    f64::from(p[axis])
                }
                _ => 0.0,
            }
        }),
        move |world: &mut World, n: f64| {
            let n = n as f32;
            let constant = match axis {
                None => Constant::Number(n),
                Some(axis) => {
                    let mut point = match value_of(
                        world.resource::<Editor>(),
                        node,
                        &input,
                    ) {
                        Some(Constant::Point(p)) => p,
                        _ => Vec3::ZERO,
                    };
                    point[axis] = n;
                    Constant::Point(point)
                }
            };
            put(
                world,
                node,
                &input.name,
                Some(Setting::Fixed(constant)),
            );
        },
    )
    .step(0.05)
    .precision(2)
    .width(px(if axis.is_some() { AXIS } else { NUMBER }))
    .rules(text_input)
    .boxed()
}

fn flag(node: NodeId, input: Input) -> AnyView {
    let read = input.clone();
    switch(
        resource::<Editor, _>(move |editor| {
            matches!(
                value_of(editor, node, &read),
                Some(Constant::Bool(true))
            )
        }),
        move |world, on| {
            put(
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
fn choice(node: NodeId, input: Input) -> AnyView {
    let read = input.clone();
    button(
        row((
            label(resource::<Editor, _>(
                move |editor| match value_of(editor, node, &read) {
                    Some(Constant::Choice(choice)) => {
                        choice.replace('_', " ")
                    }
                    _ => String::new(),
                },
            ))
            .wrap(false),
            glyph(Glyph::Chevron, MARK).tone(Tone::Dim),
        ))
        .gap(4.0)
        .align(AlignItems::Center),
    )
    .padding(bevy::ui::UiRect::axes(px(6.0), px(0.0)))
    .rules(text_input)
    .on_activate(move |world| {
        let choices = if input.assets {
            world.resource::<Editor>().rules.assets.names()
        } else {
            input.choices.iter().map(|c| (*c).to_owned()).collect()
        };
        super::menu::choose(world, node, input.name.clone(), choices);
    })
    .boxed()
}

/// Any text, for a choice with no list.
fn words(node: NodeId, input: Input) -> AnyView {
    let read = input.clone();
    text_field(
        resource::<Editor, _>(move |editor| {
            match value_of(editor, node, &read) {
                Some(Constant::Choice(text)) => text,
                _ => String::new(),
            }
        }),
        move |world, text| {
            put(
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
pub(super) fn title(at: usize) -> AnyView {
    text_field(
        resource::<Editor, _>(move |editor| {
            editor
                .sheet
                .groups
                .get(at)
                .map(|group| group.title.clone())
                .unwrap_or_default()
        }),
        move |world, text| {
            world.resource_mut::<Editor>().retitle(at, text);
        },
    )
    .width(px(NUMBER * 3.0))
    .rules(text_input)
    .boxed()
}

/// The mark that puts an input back to its default, shown while it is
/// fixed.
fn reset(node: NodeId, input: String) -> AnyView {
    let shown: Signal<Display> = {
        let input = input.clone();
        resource::<Editor, _>(move |editor| {
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
        .on_activate(move |world| put(world, node, &input, None))
        .boxed()
}
