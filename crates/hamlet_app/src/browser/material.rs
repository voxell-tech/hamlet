//! A material in the ASSETS panel's details: the shader it uses, a
//! control for each of the shader's graph inputs showing what the
//! material makes it, with a button that gives a value set back to
//! the shader's default, and an Error for each value the shader does
//! not take, with a button that removes it. Each change is written to
//! the material's file at once.

use anyhow::Context;
use bevy::app::App;
use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use bevy::math::Vec3;
use bevy::ui::{AlignItems, Display, UiRect, percent, px};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, column, icon_button,
    label, number_field, row, text_field,
};
use bevy_fynix::{ScopedExt, ViewExt, keyed, resource};
use hamlet_graph::material::{Material, fits};
use hamlet_graph::{Constant, Graph, GraphInput, ValueType};
use hamlet_graph_editor::library::picker::{picker, picture};
use hamlet_graph_editor::library::{Item, Kind, Library, lone};
use hamlet_ui::styles::{
    self, ICON, description, labelled, note, section, text_input,
};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::switch::switch;
use hamlet_ui::{AnyView, Cx, with_cx};

use crate::graphs::shader::sample::{self, Sample};

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Open>().init_resource::<Choosing>();
}

/// The height of the picker of a material's shader.
const CHOOSER: f32 = 300.0;

/// The side of the picture of the shader a material uses.
const SHOWN: f32 = 56.0;

/// The material the details show, as last read or written, and the
/// graph of its shader, for the controls to show without reading the
/// file again.
#[derive(Resource, Default)]
struct Open {
    name: String,
    material: Material,
    shader: Option<Graph>,
}

impl Open {
    /// What the input `input` is in the material: its value, else the
    /// shader's default, else the blank value of its type.
    fn value(&self, input: &str) -> Option<Constant> {
        let shader = self.shader.as_ref()?;
        let value_type = shader
            .inputs
            .iter()
            .find(|known| known.name == input)?
            .value_type;
        Some(
            self.material
                .value(shader, input)
                .unwrap_or_else(|| blank(value_type)),
        )
    }

    /// Whether the material sets the input `input` with a value it
    /// takes.
    fn sets(&self, input: &str) -> bool {
        let Some(shader) = &self.shader else {
            return false;
        };
        match (
            self.material.values.get(input),
            shader.inputs.iter().find(|known| known.name == input),
        ) {
            (Some(value), Some(known)) => {
                fits(value, known.value_type)
            }
            _ => false,
        }
    }
}

/// The details of the material `name`, built again whenever the
/// library lists something new or its shader or Errors change.
pub(super) fn details(name: String) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        // A material newly shown is not having its shader changed.
        cx.world.resource_mut::<Choosing>().0 = false;
        let name = name.clone();
        cx.build(
            keyed(
                resource::<Library, _>(|library| library.revision),
                move |_: &u64| body(name.clone()),
            )
            .within(column(()).gap(0.0)),
        )
    })
}

fn body(name: String) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let gap = cx.theme().space.gap;
        let library = cx.world.resource::<Library>();
        let read = library
            .find(Kind::Material, &name)
            .context("the material is not listed")
            .and_then(|item| item.material());
        let material = match read {
            Ok(material) => material,
            Err(error) => {
                cx.world.resource_mut::<Sample>().graph = None;
                return cx.build(column((description(format!(
                    "Error: {error:#}"
                )),)));
            }
        };
        let shader = library
            .find(Kind::Shader, &material.shader)
            .and_then(|item| item.read().ok())
            .map(|book| lone(book).graph);
        let mut rows: Vec<AnyView> = vec![
            section("SHADER").boxed(),
            shader_choice(&name, &material.shader),
        ];
        if shader.is_some() {
            rows.push(section("VALUES").boxed());
        }
        match &shader {
            Some(graph) => {
                if graph.inputs.is_empty() {
                    rows.push(
                        description(
                            "The shader has no inputs to set. Give \
                             it some in its editor.",
                        )
                        .boxed(),
                    );
                }
                for input in &graph.inputs {
                    rows.push(labelled(
                        &input.name.replace('_', " "),
                        control(&name, input),
                    ));
                }
                for error in material.errors(graph) {
                    rows.push(stale(
                        &name,
                        error.name(),
                        error.to_string(),
                    ));
                }
            }
            None if material.shader.is_empty() => {}
            None => rows.push(
                description(format!(
                    "Error: there is no shader named {}",
                    material.shader
                ))
                .boxed(),
            ),
        }
        cx.world.resource_mut::<Sample>().graph =
            shader.as_ref().map(|graph| material.apply(graph));
        cx.world.insert_resource(Open {
            name: name.clone(),
            material,
            shader,
        });
        cx.build(column(rows).gap(gap / 2.0))
    })
}

/// The shader the material `name` uses, `shader`, as its picture and
/// name with a button that changes it, or the picker of the library's
/// shaders while it uses none or it is being changed.
fn shader_choice(name: &str, shader: &str) -> AnyView {
    let (name, shader) = (name.to_owned(), shader.to_owned());
    keyed(
        resource::<Choosing, _>(|choosing| choosing.0),
        move |choosing: &bool| {
            let (name, shader) = (name.clone(), shader.clone());
            if shader.is_empty() || *choosing {
                let chosen =
                    (!shader.is_empty()).then(|| shader.clone());
                let pick = move |world: &mut World, item: &Item| {
                    world.resource_mut::<Choosing>().0 = false;
                    let picked = item.name.clone();
                    change(world, &name, true, |material| {
                        material.shader = picked;
                    });
                };
                return with_cx(move |cx| {
                    let gap = cx.theme().space.gap;
                    column((
                        description(if shader.is_empty() {
                            "Pick the shader it uses."
                        } else {
                            "Pick another shader."
                        }),
                        column((picker(
                            Kind::Shader,
                            chosen.clone(),
                            pick.clone(),
                        ),))
                        .height(px(CHOOSER))
                        .width(percent(100.0)),
                    ))
                    .gap(gap / 2.0)
                    .width(percent(100.0))
                })
                .boxed();
            }
            with_cx(move |cx| {
                let gap = cx.theme().space.gap;
                row((
                    picture(Kind::Shader, &shader, SHOWN),
                    column((
                        note(shader.replace('_', " ")),
                        button(label("Change"))
                            .padding(UiRect::axes(px(8.0), px(2.0)))
                            .rules(styles::button)
                            .tagged(Variant::Default)
                            .on_activate(|world| {
                                world.resource_mut::<Choosing>().0 =
                                    true;
                            }),
                    ))
                    .gap(gap / 2.0)
                    .align(AlignItems::FlexStart),
                ))
                .gap(gap)
                .align(AlignItems::Center)
            })
            .boxed()
        },
    )
    .within(column(()).gap(0.0).width(percent(100.0)))
    .boxed()
}

/// Whether the shader of the material shown is being changed.
#[derive(Resource, Default)]
struct Choosing(bool);

/// The control for the input `input` and the button that gives it
/// back to the shader's default while the material sets it.
fn control(name: &str, input: &GraphInput) -> AnyView {
    let reading = input.name.clone();
    let field = match input.value_type {
        ValueType::Bool => {
            let (name, input) = (name.to_owned(), input.name.clone());
            switch(
                resource::<Open, _>(move |open| {
                    matches!(
                        open.value(&reading),
                        Some(Constant::Bool(true))
                    )
                }),
                move |world, on| {
                    set(world, &name, &input, Constant::Bool(on));
                },
            )
            .boxed()
        }
        ValueType::Choice => {
            let (name, input) = (name.to_owned(), input.name.clone());
            text_field(
                resource::<Open, _>(move |open| {
                    match open.value(&reading) {
                        Some(Constant::Choice(text)) => text,
                        _ => String::new(),
                    }
                }),
                move |world, text| {
                    set(world, &name, &input, Constant::Choice(text));
                },
            )
            .width(percent(100.0))
            .rules(text_input)
            .boxed()
        }
        ValueType::Point | ValueType::Vector3 => axes(name, input, 3),
        ValueType::Vector2 => axes(name, input, 2),
        ValueType::Color => axes(name, input, 4),
        ValueType::Number | ValueType::Integer => {
            axes(name, input, 1)
        }
        _ => note("set in the shader").boxed(),
    };
    let (name, input) = (name.to_owned(), input.name.clone());
    let shown = input.clone();
    row((
        column((field,)).width(percent(100.0)).min_width(px(0.0)),
        button(glyph(Glyph::Cross, ICON))
            .display(resource::<Open, _>(move |open| {
                if open.sets(&shown) {
                    Display::Flex
                } else {
                    Display::None
                }
            }))
            .rules(icon_button)
            .tooltip(|| note("Use the shader's default"))
            .on_activate(move |world| {
                change(world, &name, false, |material| {
                    material.values.remove(&input);
                });
            }),
    ))
    .gap(4.0)
    .align(AlignItems::Center)
    .boxed()
}

/// A number field for each of the first `count` axes of the input's
/// value: one for a number, three for a point, four for a colour.
fn axes(name: &str, input: &GraphInput, count: usize) -> AnyView {
    let value_type = input.value_type;
    let fields = (0..count)
        .map(|axis| {
            let (name, input) = (name.to_owned(), input.name.clone());
            let reading = input.clone();
            number_field(
                resource::<Open, _>(move |open| {
                    open.value(&reading).map_or(0.0, |value| {
                        f64::from(get(&value, axis))
                    })
                }),
                move |world: &mut World, value: f64| {
                    let Some(mut now) =
                        world.resource::<Open>().value(&input)
                    else {
                        return;
                    };
                    let value = if value_type == ValueType::Color {
                        value.clamp(0.0, 1.0)
                    } else {
                        value
                    };
                    put(&mut now, axis, value as f32);
                    set(world, &name, &input, now);
                },
            )
            .step(if value_type == ValueType::Integer {
                1.0
            } else {
                0.05
            })
            .precision(if value_type == ValueType::Integer {
                0
            } else {
                2
            })
            .width(percent(100.0))
            .min_width(px(0.0))
            .rules(text_input)
            .boxed()
        })
        .collect::<Vec<_>>();
    row(fields).gap(4.0).align(AlignItems::Center).boxed()
}

/// The axis `axis` of `value`, a number being its own first axis.
fn get(value: &Constant, axis: usize) -> f32 {
    match value {
        Constant::Number(n) => *n,
        Constant::Integer(n) => *n as f32,
        Constant::Point(point) => point[axis],
        Constant::Color(rgba) => rgba[axis],
        _ => 0.0,
    }
}

/// Makes the axis `axis` of `value` `to`.
fn put(value: &mut Constant, axis: usize, to: f32) {
    match value {
        Constant::Number(n) => *n = to,
        Constant::Integer(n) => *n = to.round() as i32,
        Constant::Point(point) => point[axis] = to,
        Constant::Color(rgba) => rgba[axis] = to,
        _ => {}
    }
}

/// An Error about the value `input` the shader does not take, and the
/// button that removes it.
fn stale(name: &str, input: &str, error: String) -> AnyView {
    let (name, input) = (name.to_owned(), input.to_owned());
    row((
        column((description(format!("Error: {error}")),))
            .width(percent(100.0))
            .min_width(px(0.0)),
        button(glyph(Glyph::Cross, ICON))
            .rules(icon_button)
            .tooltip(|| note("Remove this value"))
            .on_activate(move |world| {
                change(world, &name, true, |material| {
                    material.values.remove(&input);
                });
            }),
    ))
    .gap(4.0)
    .align(AlignItems::Center)
    .boxed()
}

/// Sets the input `input` of the material `name` to `value`.
fn set(world: &mut World, name: &str, input: &str, value: Constant) {
    change(world, name, false, |material| {
        material.values.insert(input.to_owned(), value);
    });
}

/// Changes the material `name` as `edit` says and writes it to its
/// file, then builds the details again if `rebuild`, as when its
/// shader or its Errors change. An Error is the library's note.
fn change(
    world: &mut World,
    name: &str,
    rebuild: bool,
    edit: impl FnOnce(&mut Material),
) {
    let mut material = {
        let open = world.resource::<Open>();
        if open.name != name {
            return;
        }
        open.material.clone()
    };
    edit(&mut material);
    let mut library = world.resource_mut::<Library>();
    match library.save_material(name, &material) {
        Ok(_) => {
            if rebuild {
                library.revision += 1;
            }
        }
        Err(error) => {
            library.note = format!("{error:#}");
            return;
        }
    }
    let graph = world
        .resource::<Open>()
        .shader
        .as_ref()
        .map(|graph| material.apply(graph));
    world.resource_mut::<Sample>().graph = graph;
    world.resource_mut::<Open>().material = material;
}

/// The material on a turning sphere, while it uses a shader.
pub(super) fn preview() -> AnyView {
    column((sample::view(),))
        .width(percent(100.0))
        .display(resource::<Sample, _>(|sample| {
            if sample.graph.is_some() {
                Display::Flex
            } else {
                Display::None
            }
        }))
        .boxed()
}

/// The value an input of `value_type` takes when nothing gives it
/// one.
fn blank(value_type: ValueType) -> Constant {
    match value_type {
        ValueType::Bool => Constant::Bool(false),
        ValueType::Color => Constant::Color(bevy::math::Vec4::ONE),
        ValueType::Choice => Constant::Choice(String::new()),
        ValueType::Integer => Constant::Integer(0),
        ValueType::Point
        | ValueType::Vector2
        | ValueType::Vector3 => Constant::Point(Vec3::ZERO),
        _ => Constant::Number(0.0),
    }
}
