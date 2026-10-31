//! The open graph's own inputs, as its parameters: each one's name
//! and type, what it is when the graph above leaves it unset, and
//! whether it is picked at random then, between which numbers and how
//! often; the graph's outputs; and the sections of the flavor's own.
//! They float over the canvas.

use bevy::color::Color;
use bevy::ecs::world::World;
use bevy::math::{Vec3, Vec4};
use bevy::ui::{AlignItems, percent, px};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, column, dropdown,
    frame, icon_button, number_field, row, text_field,
};
use bevy_fynix::{ScopedExt, ViewExt, keyed, resource};
use hamlet_blueprint::{
    Constant, Edit, GraphInput, Scope, ValueType, Vary,
};
use hamlet_ui::styles::{
    ICON, command, description, note, section, text_input,
};
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::switch::switch;
use hamlet_ui::{AnyView, View, with_cx};

use super::Editor;
use super::flavor::Flavor;

/// The types an input can take, as offered.
const TYPES: [(&str, ValueType); 5] = [
    ("Number", ValueType::Number),
    ("Whole number", ValueType::Integer),
    ("Switch", ValueType::Bool),
    ("Point", ValueType::Point),
    ("Text", ValueType::Choice),
];

/// The side of a colour's swatch.
const SWATCH: f32 = 16.0;

/// The types of vector and colour, offered when some kind of the
/// flavor takes one.
const MORE: [(&str, ValueType); 3] = [
    ("Vector 2", ValueType::Vector2),
    ("Vector 3", ValueType::Vector3),
    ("Colour", ValueType::Color),
];

/// The types an input of the open graph can take, as offered.
fn types<F: Flavor>(
    editor: &Editor<F>,
) -> Vec<(&'static str, ValueType)> {
    let used = |value_type| {
        editor.registry.kinds().any(|(_, kind)| {
            kind.inputs
                .iter()
                .any(|spec| spec.value_type == value_type)
        })
    };
    TYPES
        .into_iter()
        .chain(MORE.into_iter().filter(|(_, kind)| used(*kind)))
        .collect()
}

/// How often a varying input is picked again, as offered, after
/// "Never".
const PER: [(&str, Scope); 3] = [
    ("Per item", Scope::Item),
    ("Per hamlet", Scope::Hamlet),
    ("Per realm", Scope::Realm),
];

/// The open graph's parameters, the inputs the graph above sets, each
/// with how it varies, and its outputs. Built again when the graph's
/// shape changes.
pub(super) fn view<F: Flavor>() -> impl View {
    keyed(resource::<Editor<F>, _>(|editor| editor.shape), |_| {
        panel::<F>()
    })
    .within(column(()).gap(0.0))
}

fn panel<F: Flavor>() -> AnyView {
    with_cx(|cx| {
        let editor = cx.world.resource::<Editor<F>>();
        let gap = cx.theme().space.gap;
        let mut parts: Vec<AnyView> = vec![
            section("Parameters").boxed(),
            description(
                "What the graph above sets. Left unset, each takes \
                 its default, or varies.",
            )
            .boxed(),
        ];
        for found in &editor.sheet.graph.inputs {
            parts.push(input::<F>(found.name.clone()));
        }
        parts.push(add::<F>().boxed());
        let port = |name: String,
                    value_type: ValueType,
                    edit: Edit| {
            row((
                column((note(format!("{name} ({value_type:?})")),))
                    .width(percent(100.0))
                    .min_width(px(0.0)),
                button(glyph(Glyph::Cross, ICON))
                    .rules(icon_button)
                    .tooltip(|| note("Take it away"))
                    .on_activate(move |world| {
                        world
                            .resource_mut::<Editor<F>>()
                            .apply(edit.clone());
                    }),
            ))
            .gap(gap)
            .align(AlignItems::Center)
            .boxed()
        };
        parts.push(section("Outputs").boxed());
        for output in &editor.sheet.graph.outputs {
            let edit = Edit::RemoveOutput(output.name.clone());
            parts.push(port(
                output.name.clone(),
                output.value_type,
                edit,
            ));
        }
        if editor.sheet.graph.outputs.is_empty() {
            parts.push(
                description(
                    "None. Right-click a node and Expose as output.",
                )
                .boxed(),
            );
        }
        parts.extend(F::sections());
        column(parts).gap(gap)
    })
    .boxed()
}

fn find<F: Flavor>(
    editor: &Editor<F>,
    name: &str,
) -> Option<GraphInput> {
    editor
        .sheet
        .graph
        .inputs
        .iter()
        .find(|input| input.name == name)
        .cloned()
}

/// Changes the input `name` as `change` says.
fn change<F: Flavor>(
    world: &mut World,
    name: &str,
    change: impl FnOnce(&mut GraphInput),
) {
    let mut editor = world.resource_mut::<Editor<F>>();
    let Some(mut input) = find(&editor, name) else {
        return;
    };
    change(&mut input);
    editor.apply(Edit::ChangeInput {
        name: name.to_owned(),
        input,
    });
}

/// The colour an input of colour starts from: opaque grey.
fn blank_colour() -> Vec4 {
    Vec4::new(0.5, 0.5, 0.5, 1.0)
}

/// The value an input of `value_type` starts from.
fn blank(value_type: ValueType) -> Constant {
    match value_type {
        ValueType::Bool => Constant::Bool(true),
        ValueType::Point
        | ValueType::Vector2
        | ValueType::Vector3 => Constant::Point(Vec3::ZERO),
        ValueType::Color => Constant::Color(blank_colour()),
        ValueType::Choice => Constant::Choice(String::new()),
        ValueType::Integer => Constant::Integer(0),
        _ => Constant::Number(0.0),
    }
}

/// The button that gives the graph a new number input.
pub(super) fn add<F: Flavor>() -> impl View {
    command("Add input", |world| {
        let mut editor = world.resource_mut::<Editor<F>>();
        let taken = editor
            .sheet
            .graph
            .inputs
            .iter()
            .map(|input| input.name.clone())
            .collect::<Vec<_>>();
        let name = (1..)
            .map(|n| format!("input {n}"))
            .find(|name| !taken.contains(name))
            .unwrap_or_default();
        editor.apply(Edit::AddInput(GraphInput {
            name,
            value_type: ValueType::Number,
            default: Some(Constant::Number(0.0)),
            vary: None,
        }));
    })
}

/// One input's controls.
pub(super) fn input<F: Flavor>(name: String) -> AnyView {
    with_cx(move |cx| {
        let gap = cx.theme().space.gap;
        let Some(found) =
            find(cx.world.resource::<Editor<F>>(), &name)
        else {
            return column(Vec::<AnyView>::new());
        };
        let mut parts =
            vec![head::<F>(name.clone(), found.value_type)];
        parts.push(labelled(
            "default",
            default::<F>(name.clone(), found.value_type),
        ));
        if matches!(
            found.value_type,
            ValueType::Number | ValueType::Integer
        ) {
            parts.push(labelled("varies", varies::<F>(name.clone())));
            if found.vary.is_some() {
                parts.push(labelled(
                    "between",
                    between::<F>(name.clone()),
                ));
            }
        }
        column(parts)
            .gap(gap / 2.0)
            .padding(bevy::ui::UiRect::all(px(gap)))
            .border(1.0)
            .border_color(cx.theme().palette.border)
            .radius(cx.theme().space.round)
    })
    .boxed()
}

/// `control` after a word on what it sets.
pub(super) fn labelled(word: &str, control: AnyView) -> AnyView {
    let word = word.to_owned();
    with_cx(move |cx| {
        let gap = cx.theme().space.gap;
        row((
            column((note(word.clone()),)).width(px(64.0)).shrink(0.0),
            column((control,))
                .width(percent(100.0))
                .min_width(px(0.0)),
        ))
        .gap(gap)
        .align(AlignItems::Center)
    })
    .boxed()
}

/// The input's name, its type, and a way to take it away.
fn head<F: Flavor>(name: String, value_type: ValueType) -> AnyView {
    let rename = name.clone();
    let retype = name.clone();
    let remove = name.clone();
    with_cx(move |cx| {
        let gap = cx.theme().space.gap;
        let offered = types(cx.world.resource::<Editor<F>>());
        let at = offered
            .iter()
            .position(|(_, kind)| *kind == value_type)
            .unwrap_or(0);
        let words: Vec<&str> =
            offered.iter().map(|(word, _)| *word).collect();
        let rename = rename.clone();
        let retype = retype.clone();
        let remove = remove.clone();
        row((
            text_field(name.clone(), move |world, text| {
                let text = text.trim().to_owned();
                if !text.is_empty() && text != rename {
                    change::<F>(world, &rename, |input| {
                        input.name = text;
                    });
                }
            })
            .width(percent(100.0))
            .min_width(px(0.0))
            .rules(text_input),
            dropdown(
                words,
                at,
                Glyph::Chevron.image(ICON),
                move |world, chosen| {
                    let Some(&(_, kind)) = offered.get(chosen) else {
                        return;
                    };
                    change::<F>(world, &retype, |input| {
                        if input.value_type != kind {
                            input.value_type = kind;
                            input.default = Some(blank(kind));
                            input.vary = None;
                        }
                    });
                },
            )
            .width(px(96.0))
            .shrink(0.0),
            button(glyph(Glyph::Cross, ICON))
                .rules(icon_button)
                .tooltip(|| note("Take this input away"))
                .on_activate(move |world| {
                    world
                        .resource_mut::<Editor<F>>()
                        .apply(Edit::RemoveInput(remove.clone()));
                }),
        ))
        .gap(gap / 2.0)
        .align(AlignItems::Center)
    })
    .boxed()
}

/// A number field bound to what `read` finds in the input `name`,
/// changing it as `write` says.
fn number<F: Flavor>(
    name: String,
    read: impl Fn(&GraphInput) -> f32 + Send + Sync + 'static,
    write: impl Fn(&mut GraphInput, f32) + Send + Sync + 'static,
) -> AnyView {
    let reading = name.clone();
    number_field(
        resource::<Editor<F>, _>(move |editor| {
            find(editor, &reading)
                .map_or(0.0, |input| f64::from(read(&input)))
        }),
        move |world: &mut World, value: f64| {
            change::<F>(world, &name, |input| {
                write(input, value as f32);
            });
        },
    )
    .step(0.05)
    .precision(2)
    .width(percent(100.0))
    .min_width(px(0.0))
    .rules(text_input)
    .boxed()
}

/// A field for each of the first `count` axes of the input's default,
/// a point, a vector or a colour, and for a colour a swatch of it.
fn axes<F: Flavor>(
    name: String,
    value_type: ValueType,
    count: usize,
) -> AnyView {
    let colour = value_type == ValueType::Color;
    let axis = |index: usize| {
        number::<F>(
            name.clone(),
            move |input| match input.default {
                Some(Constant::Point(p)) if !colour => p[index],
                Some(Constant::Color(c)) if colour => c[index],
                _ => 0.0,
            },
            move |input, value| {
                if colour {
                    let mut at = match input.default {
                        Some(Constant::Color(c)) => c,
                        _ => blank_colour(),
                    };
                    at[index] = value.clamp(0.0, 1.0);
                    input.default = Some(Constant::Color(at));
                } else {
                    let mut at = match input.default {
                        Some(Constant::Point(p)) => p,
                        _ => Vec3::ZERO,
                    };
                    at[index] = value;
                    input.default = Some(Constant::Point(at));
                }
            },
        )
    };
    let mut fields: Vec<AnyView> = (0..count).map(axis).collect();
    if colour {
        fields.push(swatch::<F>(name.clone()));
    }
    row(fields).gap(4.0).align(AlignItems::Center).boxed()
}

/// A small square of the colour the input's default is.
fn swatch<F: Flavor>(name: String) -> AnyView {
    let shown = resource::<Editor<F>, _>(move |editor| {
        match find(editor, &name).and_then(|input| input.default) {
            Some(Constant::Color(c)) => {
                Color::srgba(c.x, c.y, c.z, c.w)
            }
            _ => Color::NONE,
        }
    });
    with_cx(move |cx| {
        frame()
            .width(px(SWATCH))
            .height(px(SWATCH))
            .shrink(0.0)
            .radius(2.0)
            .border(1.0)
            .border_color(cx.theme().palette.border)
            .fill(shown)
    })
    .boxed()
}

/// The control for what the input is when the graph above leaves it
/// unset.
fn default<F: Flavor>(
    name: String,
    value_type: ValueType,
) -> AnyView {
    match value_type {
        ValueType::Bool => {
            let reading = name.clone();
            switch(
                resource::<Editor<F>, _>(move |editor| {
                    matches!(
                        find(editor, &reading)
                            .and_then(|input| input.default),
                        Some(Constant::Bool(true))
                    )
                }),
                move |world, on| {
                    change::<F>(world, &name, |input| {
                        input.default = Some(Constant::Bool(on));
                    });
                },
            )
            .boxed()
        }
        ValueType::Point | ValueType::Vector3 => {
            axes::<F>(name, value_type, 3)
        }
        ValueType::Vector2 => axes::<F>(name, value_type, 2),
        ValueType::Color => axes::<F>(name, value_type, 4),
        ValueType::Choice => {
            let reading = name.clone();
            text_field(
                resource::<Editor<F>, _>(move |editor| {
                    match find(editor, &reading)
                        .and_then(|input| input.default)
                    {
                        Some(Constant::Choice(text)) => text,
                        _ => String::new(),
                    }
                }),
                move |world, text| {
                    change::<F>(world, &name, |input| {
                        input.default = Some(Constant::Choice(text));
                    });
                },
            )
            .width(percent(100.0))
            .rules(text_input)
            .boxed()
        }
        ValueType::Integer => number::<F>(
            name,
            |input| match input.default {
                Some(Constant::Integer(n)) => n as f32,
                _ => 0.0,
            },
            |input, value| {
                input.default =
                    Some(Constant::Integer(value.round() as i32));
            },
        ),
        _ => number::<F>(
            name,
            |input| match input.default {
                Some(Constant::Number(n)) => n,
                _ => 0.0,
            },
            |input, value| {
                input.default = Some(Constant::Number(value));
            },
        ),
    }
}

/// Whether the input is picked at random when unset, and how often.
fn varies<F: Flavor>(name: String) -> AnyView {
    let reading = name.clone();
    dropdown(
        std::iter::once("Never").chain(PER.map(|(word, _)| word)),
        resource::<Editor<F>, _>(move |editor| {
            match find(editor, &reading).and_then(|input| input.vary)
            {
                None => 0,
                Some(vary) => {
                    1 + PER
                        .iter()
                        .position(|(_, per)| *per == vary.per)
                        .unwrap_or(0)
                }
            }
        }),
        Glyph::Chevron.image(ICON),
        move |world, at| {
            change::<F>(world, &name, |input| {
                input.vary = match at
                    .checked_sub(1)
                    .map(|at| PER[at].1)
                {
                    None => None,
                    Some(per) => {
                        let start = match input.default {
                            Some(Constant::Number(n)) => n,
                            Some(Constant::Integer(n)) => n as f32,
                            _ => 0.0,
                        };
                        let (low, high) = match input.vary {
                            Some(vary) => (vary.low, vary.high),
                            None => (start, start + 1.0),
                        };
                        Some(Vary { low, high, per })
                    }
                };
            });
        },
    )
    .width(percent(100.0))
    .boxed()
}

/// The two numbers a varying input is picked between.
fn between<F: Flavor>(name: String) -> AnyView {
    let end = |high: bool| {
        number::<F>(
            name.clone(),
            move |input| match input.vary {
                Some(vary) if high => vary.high,
                Some(vary) => vary.low,
                None => 0.0,
            },
            move |input, value| {
                if let Some(vary) = &mut input.vary {
                    if high {
                        vary.high = value;
                    } else {
                        vary.low = value;
                    }
                }
            },
        )
    };
    row((end(false), note("to"), end(true)))
        .gap(4.0)
        .align(AlignItems::Center)
        .boxed()
}
