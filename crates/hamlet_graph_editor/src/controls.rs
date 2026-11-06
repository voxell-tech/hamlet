//! The controls on a card's unwired input rows: a field for a number,
//! a whole number or each axis of a point or vector, a switch, a
//! colour's four fields with a swatch, a choice that opens its list
//! in the menu layer or the pictures of the library's meshes, and a
//! mark that puts a fixed input back to its
//! default. A path shows how many points it has, which the preview
//! edits. Changing a value fixes the input; until then it shows its
//! default. A group's title is a field on its title bar, and an
//! output's name a field on its row of the Outputs card.

use bevy::color::Color;
use bevy::ecs::world::World;
use bevy::math::Vec3;
use bevy::ui::{AlignItems, Display, Overflow, UiRect, px};
use bevy_fynix::tokens::{Motion, Tone};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, frame, icon_button,
    label, number_field, row, text_field,
};
use bevy_fynix::{ScopedExt, Signal, ViewExt, keyed, resource};
use hamlet_graph::{
    Constant, Edit, Listed, NodeId, Setting, ValueType,
};
use hamlet_ui::AnyView;
use hamlet_ui::styles::{note, swatch, text_input};
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::switch::switch;

use super::Editor;
use super::canvas::Input;
use super::library::picker;
use super::menu::Menu;
use super::setup::EditorKey;

/// The width of a number's field.
const NUMBER: f32 = 58.0;
/// The width of each axis of a point's fields.
const AXIS: f32 = 38.0;
/// The width of each of a colour's four fields.
const CHANNEL: f32 = 32.0;
/// The side of a swatch or a glyph beside a field.
const MARK: f32 = 12.0;
/// The widest a choice's control grows before its words are cut.
const CHOICE: f32 = 140.0;
/// The side of the picture of an asset chosen.
const PICTURE: f32 = 20.0;

/// How an input of `node` is set now.
pub(super) fn current<K: EditorKey>(
    editor: &Editor<K>,
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
pub(super) fn value_of<K: EditorKey>(
    editor: &Editor<K>,
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

pub(super) fn put<K: EditorKey>(
    world: &mut World,
    node: NodeId,
    input: &str,
    setting: Option<Setting>,
) {
    world.resource_mut::<Editor<K>>().apply(Edit::Set {
        node,
        input: input.to_owned(),
        setting,
    });
}

/// The controls for an unwired input of `node`, as it is set when the
/// card is built, or nothing for one that can only be wired.
pub(super) fn control<K: EditorKey>(
    node: NodeId,
    input: Input,
    setting: Option<&Setting>,
) -> Option<AnyView> {
    if input.value_type == ValueType::Path {
        return Some(points::<K>(node, input));
    }
    let value = match setting {
        Some(Setting::Random(_)) => {
            return Some(pill("random".into()));
        }
        _ => match written(&input)? {
            ValueType::Number | ValueType::Integer => {
                number::<K>(node, input.clone(), Slot::Scalar)
            }
            ValueType::Point | ValueType::Vector3 => {
                axes::<K>(node, &input, 3)
            }
            ValueType::Vector2 => axes::<K>(node, &input, 2),
            ValueType::Color => row((
                axes::<K>(node, &input, 4),
                swatch_of::<K>(node, input.clone()),
            ))
            .gap(2.0)
            .align(AlignItems::Center)
            .boxed(),
            ValueType::Bool => flag::<K>(node, input.clone()),
            _ if !input.choices.is_empty()
                || input.listed.is_some() =>
            {
                choice::<K>(node, input.clone())
            }
            _ => words::<K>(node, input.clone()),
        },
    };
    Some(
        row((value, reset::<K>(node, input.name)))
            .gap(2.0)
            .align(AlignItems::Center)
            .boxed(),
    )
}

/// The note on a path input: how many points it has, and the mark
/// that puts them back to the default.
fn points<K: EditorKey>(node: NodeId, input: Input) -> AnyView {
    let read = input.clone();
    row((
        label(resource::<Editor<K>, _>(
            move |editor| match value_of(editor, node, &read) {
                Some(Constant::Path(path)) if path.len() == 1 => {
                    "1 point".to_owned()
                }
                Some(Constant::Path(path)) => {
                    format!("{} points", path.len())
                }
                _ => String::new(),
            },
        ))
        .wrap(false)
        .tone(Tone::Dim),
        reset::<K>(node, input.name),
    ))
    .gap(2.0)
    .align(AlignItems::Center)
    .boxed()
}

/// What sets an input other than a value of its own or a wire, in
/// place of its controls.
fn pill(text: String) -> AnyView {
    label(text).wrap(false).tone(Tone::Accent).boxed()
}

/// A row of fields, one for each of the first `count` axes of a
/// point, a vector or a colour.
fn axes<K: EditorKey>(
    node: NodeId,
    input: &Input,
    count: usize,
) -> AnyView {
    row((0..count)
        .map(|axis| {
            number::<K>(node, input.clone(), Slot::Axis(axis))
        })
        .collect::<Vec<_>>())
    .gap(2.0)
    .boxed()
}

/// The part of an input's value a number field sets.
#[derive(Clone, Copy)]
enum Slot {
    /// The whole value, a number or a whole number.
    Scalar,
    /// One axis of a point, a vector or a colour.
    Axis(usize),
}

/// A field for a number, whole or not, or for one axis of a point, a
/// vector or a colour.
fn number<K: EditorKey>(
    node: NodeId,
    input: Input,
    slot: Slot,
) -> AnyView {
    let read = input.clone();
    let whole = input.value_type == ValueType::Integer;
    let colour = input.value_type == ValueType::Color;
    let field = number_field(
        resource::<Editor<K>, _>(move |editor| {
            match (value_of(editor, node, &read), slot) {
                (Some(Constant::Number(value)), Slot::Scalar) => {
                    f64::from(value)
                }
                (Some(Constant::Integer(value)), Slot::Scalar) => {
                    f64::from(value)
                }
                // A graph read from a file may hold a value of
                // another shape than the input's, with no such axis.
                (Some(Constant::Point(point)), Slot::Axis(axis)) => {
                    point
                        .to_array()
                        .get(axis)
                        .map_or(0.0, |v| f64::from(*v))
                }
                (Some(Constant::Color(rgba)), Slot::Axis(axis)) => {
                    rgba.to_array()
                        .get(axis)
                        .map_or(0.0, |v| f64::from(*v))
                }
                _ => 0.0,
            }
        }),
        move |world: &mut World, value: f64| {
            let value = value as f32;
            let constant = match slot {
                Slot::Scalar if whole => {
                    Constant::Integer(value.round() as i32)
                }
                Slot::Scalar => Constant::Number(value),
                Slot::Axis(axis) => {
                    let current = value_of(
                        world.resource::<Editor<K>>(),
                        node,
                        &input,
                    );
                    match current {
                        Some(Constant::Color(mut rgba))
                            if axis < 4 =>
                        {
                            rgba[axis] = value;
                            Constant::Color(rgba)
                        }
                        Some(Constant::Point(mut point))
                            if axis < 3 =>
                        {
                            point[axis] = value;
                            Constant::Point(point)
                        }
                        _ if axis < 3 => {
                            let mut point = Vec3::ZERO;
                            point[axis] = value;
                            Constant::Point(point)
                        }
                        _ => return,
                    }
                }
            };
            put::<K>(
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
    let width = match (slot, colour) {
        (Slot::Scalar, _) => NUMBER,
        (Slot::Axis(_), false) => AXIS,
        (Slot::Axis(_), true) => CHANNEL,
    };
    field.width(px(width)).rules(text_input).boxed()
}

/// A small square of the colour an input is set to.
fn swatch_of<K: EditorKey>(node: NodeId, input: Input) -> AnyView {
    let shown =
        resource::<Editor<K>, _>(move |editor| {
            match value_of(editor, node, &input) {
                Some(Constant::Color(rgba)) => {
                    Color::srgba(rgba.x, rgba.y, rgba.z, rgba.w)
                }
                _ => Color::NONE,
            }
        });
    swatch(MARK, shown)
}

fn flag<K: EditorKey>(node: NodeId, input: Input) -> AnyView {
    let read = input.clone();
    switch(
        resource::<Editor<K>, _>(move |editor| {
            matches!(
                value_of(editor, node, &read),
                Some(Constant::Bool(true))
            )
        }),
        move |world, on| {
            put::<K>(
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
/// at the pointer, or the pictures of the library's meshes for an
/// asset, which it then shows a picture of too. The chevron points
/// down, and up while the list is open.
fn choice<K: EditorKey>(node: NodeId, input: Input) -> AnyView {
    let read = input.clone();
    let name = input.name.clone();
    let pictured = input.clone();
    let picture = keyed(
        resource::<Editor<K>, _>(move |editor| {
            match (pictured.listed, value_of(editor, node, &pictured))
            {
                (
                    Some(Listed::Assets),
                    Some(Constant::Choice(asset)),
                ) => Some(asset),
                _ => None,
            }
        }),
        |asset: &Option<String>| match asset {
            Some(asset) => picker::picture(asset, PICTURE),
            None => frame().boxed(),
        },
    )
    .within(row(()).gap(0.0));
    button(
        row((
            picture,
            // Words too long for the control are cut at its edge, so
            // the chevron keeps its place.
            row((label(resource::<Editor<K>, _>(move |editor| {
                match value_of(editor, node, &read) {
                    Some(Constant::Choice(choice)) => {
                        choice.replace('_', " ")
                    }
                    _ => String::new(),
                }
            }))
            .wrap(false),))
            .grow(1.0)
            .min_width(px(0.0))
            .overflow(Overflow::clip()),
            glyph(Glyph::Chevron, MARK)
                .tone(Tone::Dim)
                .rotation(resource::<Menu<K>, _>(move |menu| {
                    if menu.is_choosing(node, &name) {
                        0.0
                    } else {
                        180.0
                    }
                }))
                .transition(Motion::Interact),
        ))
        .gap(4.0)
        .align(AlignItems::Center)
        .min_width(px(0.0)),
    )
    .padding(UiRect::axes(px(6.0), px(0.0)))
    .max_width(px(CHOICE))
    .min_width(px(0.0))
    .rules(text_input)
    .on_activate(move |world| {
        if input.listed == Some(Listed::Assets) {
            super::menu::choose_asset::<K>(
                world,
                node,
                input.name.clone(),
            );
            return;
        }
        let catalogue = &world.resource::<Editor<K>>().rules.assets;
        let choices = match input.listed {
            Some(Listed::Sockets) => catalogue.socket_names(),
            Some(Listed::Assets) => Vec::new(),
            None => input
                .choices
                .iter()
                .map(|choice| (*choice).to_owned())
                .collect(),
        };
        super::menu::choose::<K>(
            world,
            node,
            input.name.clone(),
            choices,
        );
    })
    .boxed()
}

/// Any text, for a choice with no list.
fn words<K: EditorKey>(node: NodeId, input: Input) -> AnyView {
    let read = input.clone();
    text_field(
        resource::<Editor<K>, _>(move |editor| {
            match value_of(editor, node, &read) {
                Some(Constant::Choice(text)) => text,
                _ => String::new(),
            }
        }),
        move |world, text| {
            put::<K>(
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
pub(super) fn title<K: EditorKey>(at: usize) -> AnyView {
    text_field(
        resource::<Editor<K>, _>(move |editor| {
            editor
                .sheet
                .groups
                .get(at)
                .map(|group| group.title.clone())
                .unwrap_or_default()
        }),
        move |world, text| {
            world.resource_mut::<Editor<K>>().retitle(at, text);
        },
    )
    .width(px(NUMBER * 3.0))
    .rules(text_input)
    .boxed()
}

/// The field the name of the output `name` is written in on its row
/// of the Outputs card.
pub(super) fn output_name<K: EditorKey>(name: String) -> AnyView {
    let rename = name.clone();
    text_field(name, move |world, text| {
        world
            .resource_mut::<Editor<K>>()
            .rename_output(&rename, text);
    })
    .grow(1.0)
    .min_width(px(0.0))
    .rules(text_input)
    .boxed()
}

/// The mark that puts an input back to its default, shown while it is
/// fixed.
fn reset<K: EditorKey>(node: NodeId, input: String) -> AnyView {
    let shown: Signal<Display> = {
        let input = input.clone();
        resource::<Editor<K>, _>(move |editor| {
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
        .on_activate(move |world| put::<K>(world, node, &input, None))
        .boxed()
}
