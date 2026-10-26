//! The selected node's inputs: how each is set, and its values.

use bevy::ecs::world::World;
use bevy::math::Vec3;
use bevy::ui::{AlignItems, percent, px};
use bevy_fynix::views::{
    FrameProps, column, dropdown, number_field, row, text_field,
};
use bevy_fynix::{ScopedExt, keyed, resource};
use hamlet_graph::{
    Constant, Does, Edit, Initial, NodeId, PortSpec, Roll, Scope,
    Setting, Spread, ValueType,
};

use super::Editor;
use crate::icons::Glyph;
use crate::looks::{ICON, description, note, section, well};
use crate::switch::switch;
use crate::{AnyView, View, with_cx};

/// The ways an input can be set, in the order offered.
const MODES: [&str; 4] =
    ["Default", "Fixed", "Random", "From parent"];

const SCOPES: [(&str, Scope); 4] = [
    ("Per realm", Scope::Realm),
    ("Per hamlet", Scope::Hamlet),
    ("Per house", Scope::House),
    ("Per placement", Scope::Placement),
];

/// The width of a value's control.
const CONTROL: f32 = 150.0;

pub(super) fn view() -> impl View {
    keyed(
        resource::<Editor, _>(|editor| {
            (editor.selected, editor.shape)
        }),
        |(selected, _)| match selected {
            Some(node) => node_view(*node),
            None => AnyView::new(|cx| {
                cx.build(note("Select a node to set its inputs."))
            }),
        },
    )
    .within(column(()).gap(0.0))
}

fn node_view(node: NodeId) -> AnyView {
    with_cx(move |cx| {
        let editor = cx.world.resource::<Editor>();
        let gap = cx.theme().space.gap;
        let Some(found) = editor.sheet.graph.node(node) else {
            return column(Vec::<AnyView>::new());
        };
        let (title, doc, specs) = match &found.does {
            Does::Kind(name) => match editor.registry.get(name) {
                Some(kind) => (
                    kind.name.replace('_', " "),
                    kind.doc,
                    kind.inputs,
                ),
                None => (name.clone(), "An unknown kind", &[][..]),
            },
            Does::Graph(name) => {
                (name.clone(), "Another graph", &[][..])
            }
        };
        let mut parts: Vec<AnyView> = vec![
            AnyView::new(move |cx| cx.build(section(title.clone()))),
            AnyView::new(move |cx| cx.build(description(doc))),
        ];
        for spec in specs {
            let wired = editor.sheet.graph.wires.iter().any(|wire| {
                wire.to.node == node && wire.to.name == spec.name
            });
            parts.push(input(node, *spec, wired));
        }
        column(parts).gap(gap)
    })
}

/// One input: its name, how it is set, and the controls for its
/// value.
fn input(node: NodeId, spec: PortSpec, wired: bool) -> AnyView {
    with_cx(move |cx| {
        let gap = cx.theme().space.gap;
        let name = spec.name.replace('_', " ");
        let head = row((
            column((note(name),))
                .width(percent(100.0))
                .min_width(px(0.0)),
            mode(node, spec, wired),
        ))
        .gap(gap)
        .align(AlignItems::Center);
        let current =
            current(cx.world.resource::<Editor>(), node, spec.name);
        let body = if wired {
            None
        } else {
            value(node, spec, current.as_ref())
        };
        let mut parts: Vec<AnyView> =
            vec![AnyView::new(move |cx| cx.build(head))];
        parts.extend(body);
        column(parts).gap(gap / 2.0)
    })
}

/// How an input of `node` is set now.
fn current(
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

fn put(
    world: &mut World,
    node: NodeId,
    input: &'static str,
    setting: Option<Setting>,
) {
    world.resource_mut::<Editor>().apply(Edit::Set {
        node,
        input: input.to_owned(),
        setting,
    });
}

/// The type an input's setting is written in. A field is set as a
/// number, the same everywhere.
fn written(spec: &PortSpec) -> Option<ValueType> {
    match spec.value_type {
        ValueType::Field | ValueType::Number => {
            Some(ValueType::Number)
        }
        ValueType::Bool => Some(ValueType::Bool),
        ValueType::Point => Some(ValueType::Point),
        ValueType::Choice => Some(ValueType::Choice),
        _ => None,
    }
}

/// The constant an input starts from when it is fixed.
fn start(spec: &PortSpec) -> Option<Constant> {
    Some(match (spec.initial, written(spec)?) {
        (Some(Initial::Number(n)), _) => Constant::Number(n),
        (Some(Initial::Bool(on)), _) => Constant::Bool(on),
        (Some(Initial::Point(p)), _) => Constant::Point(p),
        (Some(Initial::Choice(c)), _) => {
            Constant::Choice(c.to_owned())
        }
        (None, ValueType::Bool) => Constant::Bool(true),
        (None, ValueType::Point) => Constant::Point(Vec3::ZERO),
        (None, ValueType::Choice) => Constant::Choice(
            spec.choices
                .first()
                .copied()
                .unwrap_or_default()
                .to_owned(),
        ),
        (None, _) => Constant::Number(0.0),
    })
}

/// The dropdown of how an input is set.
fn mode(node: NodeId, spec: PortSpec, wired: bool) -> impl View {
    let options = if wired {
        vec!["Wired"]
    } else if written(&spec).is_none() {
        vec!["Wire only"]
    } else {
        MODES.to_vec()
    };
    let fixed = !wired && written(&spec).is_some();
    dropdown(
        options,
        resource::<Editor, _>(move |editor| {
            if !fixed {
                return 0;
            }
            match current(editor, node, spec.name) {
                None => 0,
                Some(Setting::Fixed(_)) => 1,
                Some(Setting::Random(_)) => 2,
                Some(Setting::FromParent(_)) => 3,
            }
        }),
        Glyph::Chevron.image(ICON),
        move |world, at| {
            if !fixed {
                return;
            }
            let setting = match at {
                1 => start(&spec).map(Setting::Fixed),
                2 => random(&spec),
                3 => {
                    let editor = world.resource::<Editor>();
                    let accepts = spec.value_type;
                    editor
                        .sheet
                        .graph
                        .inputs
                        .iter()
                        .find(|input| {
                            accepts.accepts(input.value_type)
                        })
                        .map(|input| {
                            Setting::FromParent(input.name.clone())
                        })
                }
                _ => None,
            };
            if at == 0 || setting.is_some() {
                put(world, node, spec.name, setting);
            }
        },
    )
    .width(px(CONTROL))
    .shrink(0.0)
    .rules(well)
}

/// A random setting to start from for `spec`.
fn random(spec: &PortSpec) -> Option<Setting> {
    let spread = match (written(spec)?, start(spec)?) {
        (ValueType::Bool, _) => Spread::Chance(0.5),
        (ValueType::Choice, _) if !spec.choices.is_empty() => {
            Spread::Weights(
                spec.choices
                    .iter()
                    .map(|choice| ((*choice).to_owned(), 1.0))
                    .collect(),
            )
        }
        (ValueType::Number, Constant::Number(n)) => {
            Spread::Range(n, n + 1.0)
        }
        _ => return None,
    };
    Some(Setting::Random(Roll {
        spread,
        scope: Scope::Placement,
    }))
}

/// The controls for the value of an input set as it is now.
fn value(
    node: NodeId,
    spec: PortSpec,
    setting: Option<&Setting>,
) -> Option<AnyView> {
    let name = spec.name;
    Some(match setting? {
        Setting::Fixed(Constant::Number(_)) => boxed(number(
            node,
            name,
            |setting| match setting {
                Some(Setting::Fixed(Constant::Number(n))) => {
                    Some(f64::from(*n))
                }
                _ => None,
            },
            |_, n| Setting::Fixed(Constant::Number(n)),
        )),
        Setting::Fixed(Constant::Bool(_)) => {
            let on = resource::<Editor, _>(move |editor| {
                matches!(
                    current(editor, node, name),
                    Some(Setting::Fixed(Constant::Bool(true)))
                )
            });
            boxed(switch(on, move |world, on| {
                put(
                    world,
                    node,
                    name,
                    Some(Setting::Fixed(Constant::Bool(on))),
                );
            }))
        }
        Setting::Fixed(Constant::Choice(_))
            if !spec.choices.is_empty() =>
        {
            let choices = spec.choices;
            boxed(
                dropdown(
                    choices
                        .iter()
                        .map(|choice| choice.replace('_', " ")),
                    resource::<Editor, _>(
                        move |editor| match current(
                            editor, node, name,
                        ) {
                            Some(Setting::Fixed(
                                Constant::Choice(choice),
                            )) => choices
                                .iter()
                                .position(|c| *c == choice)
                                .unwrap_or(0),
                            _ => 0,
                        },
                    ),
                    Glyph::Chevron.image(ICON),
                    move |world, at| {
                        let choice = choices[at].to_owned();
                        put(
                            world,
                            node,
                            name,
                            Some(Setting::Fixed(Constant::Choice(
                                choice,
                            ))),
                        );
                    },
                )
                .width(percent(100.0))
                .min_width(px(0.0))
                .rules(well),
            )
        }
        Setting::Fixed(Constant::Choice(_)) => boxed(
            text_field(
                resource::<Editor, _>(move |editor| {
                    match current(editor, node, name) {
                        Some(Setting::Fixed(Constant::Choice(
                            choice,
                        ))) => choice,
                        _ => String::new(),
                    }
                }),
                move |world, text| {
                    put(
                        world,
                        node,
                        name,
                        Some(Setting::Fixed(Constant::Choice(text))),
                    );
                },
            )
            .width(percent(100.0))
            .min_width(px(0.0))
            .rules(well),
        ),
        Setting::Fixed(Constant::Point(_)) => {
            let axis = |index: usize| {
                number(
                    node,
                    name,
                    move |setting| match setting {
                        Some(Setting::Fixed(Constant::Point(p))) => {
                            Some(f64::from(p[index]))
                        }
                        _ => None,
                    },
                    move |setting, n| {
                        let mut point = match setting {
                            Some(Setting::Fixed(
                                Constant::Point(p),
                            )) => *p,
                            _ => Vec3::ZERO,
                        };
                        point[index] = n;
                        Setting::Fixed(Constant::Point(point))
                    },
                )
            };
            boxed(row((axis(0), axis(1), axis(2))).gap(4.0))
        }
        Setting::Random(roll) => {
            let bounds = match roll.spread {
                Spread::Range(..) => {
                    let end = |high: bool| {
                        number(
                            node,
                            name,
                            move |setting| match setting {
                                Some(Setting::Random(Roll {
                                    spread: Spread::Range(low, top),
                                    ..
                                })) => Some(f64::from(if high {
                                    *top
                                } else {
                                    *low
                                })),
                                _ => None,
                            },
                            move |setting, n| {
                                let Some(Setting::Random(mut roll)) =
                                    setting.cloned()
                                else {
                                    return Setting::Random(Roll {
                                        spread: Spread::Range(n, n),
                                        scope: Scope::Placement,
                                    });
                                };
                                if let Spread::Range(low, top) =
                                    &mut roll.spread
                                {
                                    *if high { top } else { low } = n;
                                }
                                Setting::Random(roll)
                            },
                        )
                    };
                    boxed(
                        row((
                            note("from"),
                            end(false),
                            note("to"),
                            end(true),
                        ))
                        .gap(4.0)
                        .align(AlignItems::Center),
                    )
                }
                Spread::Chance(_) => boxed(
                    row((
                        note("chance"),
                        number(
                            node,
                            name,
                            |setting| match setting {
                                Some(Setting::Random(Roll {
                                    spread: Spread::Chance(chance),
                                    ..
                                })) => Some(f64::from(*chance)),
                                _ => None,
                            },
                            |setting, n| {
                                let scope = match setting {
                                    Some(Setting::Random(roll)) => {
                                        roll.scope
                                    }
                                    _ => Scope::Placement,
                                };
                                Setting::Random(Roll {
                                    spread: Spread::Chance(
                                        n.clamp(0.0, 1.0),
                                    ),
                                    scope,
                                })
                            },
                        ),
                    ))
                    .gap(4.0)
                    .align(AlignItems::Center),
                ),
                Spread::Weights(_) => {
                    boxed(note("Each choice as likely"))
                }
            };
            boxed(column((bounds, scope(node, name))).gap(4.0))
        }
        Setting::FromParent(input) => {
            boxed(note(format!("Set by the graph above, as {input}")))
        }
    })
}

fn boxed(view: impl View) -> AnyView {
    AnyView::new(move |cx| cx.build(view))
}

/// A number field showing what `read` finds in the setting, and
/// setting what `write` makes of a new number.
fn number(
    node: NodeId,
    name: &'static str,
    read: impl Fn(Option<&Setting>) -> Option<f64>
    + Send
    + Sync
    + Copy
    + 'static,
    write: impl Fn(Option<&Setting>, f32) -> Setting
    + Send
    + Sync
    + 'static,
) -> impl View {
    number_field(
        resource::<Editor, _>(move |editor| {
            read(current(editor, node, name).as_ref())
                .unwrap_or_default()
        }),
        move |world: &mut World, value: f64| {
            let setting =
                current(world.resource::<Editor>(), node, name);
            put(
                world,
                node,
                name,
                Some(write(setting.as_ref(), value as f32)),
            );
        },
    )
    .step(0.05)
    .precision(2)
    .width(percent(100.0))
    .min_width(px(0.0))
    .rules(well)
}

/// The dropdown of how often a random input is rolled again.
fn scope(node: NodeId, name: &'static str) -> impl View {
    dropdown(
        SCOPES.map(|(text, _)| text),
        resource::<Editor, _>(move |editor| {
            match current(editor, node, name) {
                Some(Setting::Random(roll)) => SCOPES
                    .iter()
                    .position(|(_, scope)| *scope == roll.scope)
                    .unwrap_or(3),
                _ => 3,
            }
        }),
        Glyph::Chevron.image(ICON),
        move |world, at| {
            if let Some(Setting::Random(mut roll)) =
                current(world.resource::<Editor>(), node, name)
            {
                roll.scope = SCOPES[at].1;
                put(world, node, name, Some(Setting::Random(roll)));
            }
        },
    )
    .width(percent(100.0))
    .min_width(px(0.0))
    .rules(well)
}
