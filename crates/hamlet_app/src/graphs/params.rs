//! The open graph's own inputs, as its parameters: each one's name
//! and type, what it is when the graph above leaves it unset, and
//! whether it is picked at random then, between which numbers and how
//! often.

use bevy::ecs::world::World;
use bevy::math::Vec3;
use bevy::ui::{AlignItems, percent, px};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, column, dropdown,
    icon_button, number_field, row, text_field,
};
use bevy_fynix::{ScopedExt, ViewExt, resource};
use hamlet_proc::{
    Constant, Edit, GraphInput, Scope, ValueType, Vary,
};

use super::Editor;
use crate::icons::{Glyph, glyph};
use crate::looks::{ICON, command, note, well};
use crate::switch::switch;
use crate::{AnyView, View, with_cx};

/// The types an input can take, as offered.
const TYPES: [(&str, ValueType); 4] = [
    ("Number", ValueType::Number),
    ("Switch", ValueType::Bool),
    ("Point", ValueType::Point),
    ("Text", ValueType::Choice),
];

/// How often a varying input is picked again, as offered, after
/// "Never".
const PER: [(&str, Scope); 3] = [
    ("Per house", Scope::House),
    ("Per hamlet", Scope::Hamlet),
    ("Per realm", Scope::Realm),
];

fn find(editor: &Editor, name: &str) -> Option<GraphInput> {
    editor
        .sheet
        .graph
        .inputs
        .iter()
        .find(|input| input.name == name)
        .cloned()
}

/// Changes the input `name` as `change` says.
fn change(
    world: &mut World,
    name: &str,
    change: impl FnOnce(&mut GraphInput),
) {
    let mut editor = world.resource_mut::<Editor>();
    let Some(mut input) = find(&editor, name) else {
        return;
    };
    change(&mut input);
    editor.apply(Edit::ChangeInput {
        name: name.to_owned(),
        input,
    });
}

/// The value an input of `value_type` starts from.
fn blank(value_type: ValueType) -> Constant {
    match value_type {
        ValueType::Bool => Constant::Bool(true),
        ValueType::Point => Constant::Point(Vec3::ZERO),
        ValueType::Choice => Constant::Choice(String::new()),
        _ => Constant::Number(0.0),
    }
}

/// The button that gives the graph a new number input.
pub(super) fn add() -> impl View {
    command("Add input", |world| {
        let mut editor = world.resource_mut::<Editor>();
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
pub(super) fn input(name: String) -> AnyView {
    with_cx(move |cx| {
        let gap = cx.theme().space.gap;
        let Some(found) = find(cx.world.resource::<Editor>(), &name)
        else {
            return column(Vec::<AnyView>::new());
        };
        let mut parts = vec![head(name.clone(), found.value_type)];
        parts.push(labelled(
            "default",
            default(name.clone(), found.value_type),
        ));
        if found.value_type == ValueType::Number {
            parts.push(labelled("varies", varies(name.clone())));
            if found.vary.is_some() {
                parts
                    .push(labelled("between", between(name.clone())));
            }
        }
        column(parts)
            .gap(gap / 2.0)
            .padding(bevy::ui::UiRect::all(px(gap)))
            .border(1.0)
            .border_color(cx.theme().palette.seam)
            .radius(cx.theme().space.round)
    })
}

/// `control` after a word on what it sets.
fn labelled(word: &'static str, control: AnyView) -> AnyView {
    with_cx(move |cx| {
        let gap = cx.theme().space.gap;
        row((
            column((note(word),)).width(px(64.0)).shrink(0.0),
            column((control,))
                .width(percent(100.0))
                .min_width(px(0.0)),
        ))
        .gap(gap)
        .align(AlignItems::Center)
    })
}

/// The input's name, its type, and a way to take it away.
fn head(name: String, value_type: ValueType) -> AnyView {
    let rename = name.clone();
    let retype = name.clone();
    let remove = name.clone();
    with_cx(move |cx| {
        let gap = cx.theme().space.gap;
        let at = TYPES
            .iter()
            .position(|(_, kind)| *kind == value_type)
            .unwrap_or(0);
        let rename = rename.clone();
        let retype = retype.clone();
        let remove = remove.clone();
        row((
            text_field(name.clone(), move |world, text| {
                let text = text.trim().to_owned();
                if !text.is_empty() && text != rename {
                    change(world, &rename, |input| input.name = text);
                }
            })
            .width(percent(100.0))
            .min_width(px(0.0))
            .rules(well),
            dropdown(
                TYPES.map(|(word, _)| word),
                at,
                Glyph::Chevron.image(ICON),
                move |world, chosen| {
                    let kind = TYPES[chosen].1;
                    change(world, &retype, |input| {
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
                        .resource_mut::<Editor>()
                        .apply(Edit::RemoveInput(remove.clone()));
                }),
        ))
        .gap(gap / 2.0)
        .align(AlignItems::Center)
    })
}

/// A number field bound to what `read` finds in the input `name`,
/// changing it as `write` says.
fn number(
    name: String,
    read: impl Fn(&GraphInput) -> f32 + Send + Sync + 'static,
    write: impl Fn(&mut GraphInput, f32) + Send + Sync + 'static,
) -> AnyView {
    let reading = name.clone();
    number_field(
        resource::<Editor, _>(move |editor| {
            find(editor, &reading)
                .map_or(0.0, |input| f64::from(read(&input)))
        }),
        move |world: &mut World, value: f64| {
            change(world, &name, |input| write(input, value as f32));
        },
    )
    .step(0.05)
    .precision(2)
    .width(percent(100.0))
    .min_width(px(0.0))
    .rules(well)
    .boxed()
}

/// The control for what the input is when the graph above leaves it
/// unset.
fn default(name: String, value_type: ValueType) -> AnyView {
    match value_type {
        ValueType::Bool => {
            let reading = name.clone();
            switch(
                resource::<Editor, _>(move |editor| {
                    matches!(
                        find(editor, &reading)
                            .and_then(|input| input.default),
                        Some(Constant::Bool(true))
                    )
                }),
                move |world, on| {
                    change(world, &name, |input| {
                        input.default = Some(Constant::Bool(on));
                    });
                },
            )
            .boxed()
        }
        ValueType::Point => {
            let axis = |index: usize| {
                number(
                    name.clone(),
                    move |input| match input.default {
                        Some(Constant::Point(p)) => p[index],
                        _ => 0.0,
                    },
                    move |input, value| {
                        let mut point = match input.default {
                            Some(Constant::Point(p)) => p,
                            _ => Vec3::ZERO,
                        };
                        point[index] = value;
                        input.default = Some(Constant::Point(point));
                    },
                )
            };
            row((axis(0), axis(1), axis(2))).gap(4.0).boxed()
        }
        ValueType::Choice => {
            let reading = name.clone();
            text_field(
                resource::<Editor, _>(move |editor| {
                    match find(editor, &reading)
                        .and_then(|input| input.default)
                    {
                        Some(Constant::Choice(text)) => text,
                        _ => String::new(),
                    }
                }),
                move |world, text| {
                    change(world, &name, |input| {
                        input.default = Some(Constant::Choice(text));
                    });
                },
            )
            .width(percent(100.0))
            .rules(well)
            .boxed()
        }
        _ => number(
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
fn varies(name: String) -> AnyView {
    let reading = name.clone();
    dropdown(
        std::iter::once("Never").chain(PER.map(|(word, _)| word)),
        resource::<Editor, _>(move |editor| {
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
            change(world, &name, |input| {
                input.vary =
                    match at.checked_sub(1).map(|at| PER[at].1) {
                        None => None,
                        Some(per) => {
                            let start = match input.default {
                                Some(Constant::Number(n)) => n,
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
fn between(name: String) -> AnyView {
    let end = |high: bool| {
        number(
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
