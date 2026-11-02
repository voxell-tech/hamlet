//! The targets the selected cards offer the gizmo: a transform node's
//! translation, rotation and scale; a curve's ends or middle to move;
//! a polygon's or circle's radius or a rectangle's sides to pull; and
//! a spline's points, with the ends of their tangent handles. An
//! input wired from a parameter node moves the parameter's default;
//! one wired from anything else offers nothing.

use bevy::math::{EulerRot, Quat, Vec3};
use hamlet_blueprint::spline::SplineKind;
use hamlet_graph::{
    Constant, Does, Edit, Graph, GraphInput, NodeId, PathPoint,
    Setting,
};
use hamlet_graph_editor::Editor;

use super::Blueprint;

/// One thing the gizmo can take hold of: where it is, and what it
/// sets.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Target {
    pub(super) node: NodeId,
    pub(super) at: Vec3,
    pub(super) kind: Kind,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Kind {
    /// A transform node: its translation is `at`, and it turns and
    /// scales.
    Transform { rotation: Quat, scale: Vec3 },
    /// A point input, which only moves.
    Point { input: &'static str },
    /// A number that is the distance along `axis` from `middle` to
    /// `at`, times `times`: a radius, or a side twice as far.
    Distance {
        input: &'static str,
        middle: Vec3,
        axis: Vec3,
        times: f32,
    },
    /// A spline's point `index`, or the end of one of its tangent
    /// handles, which is `at` and starts from `anchor`.
    Knot {
        index: usize,
        part: Part,
        anchor: Vec3,
    },
}

/// Which of a spline's point and its two handles a knot is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Part {
    Anchor,
    Before,
    After,
}

/// The input of a spline node that holds its points.
pub(super) const POINTS: &str = "points";

/// A spline node's points and how they are followed.
pub(super) struct Spline {
    pub(super) points: Vec<PathPoint>,
    pub(super) kind: SplineKind,
    pub(super) closed: bool,
}

/// The targets of every selected card, in the order they are
/// selected.
pub(super) fn targets(editor: &Editor<Blueprint>) -> Vec<Target> {
    let mut found = Vec::new();
    for node in &editor.selected {
        let Some(card) = editor.sheet.graph.node(*node) else {
            continue;
        };
        let Does::Kind(kind) = &card.does else {
            continue;
        };
        let point =
            |input: &'static str| point_of(editor, *node, input);
        let number =
            |input: &'static str| number_of(editor, *node, input);
        let mut add = |at: Vec3, kind: Kind| {
            found.push(Target {
                node: *node,
                at,
                kind,
            });
        };
        match kind.as_str() {
            "transform" => {
                if let Some(at) = point("translation") {
                    let turn =
                        point("rotation").unwrap_or(Vec3::ZERO);
                    add(
                        at,
                        Kind::Transform {
                            rotation: rotation_of(turn),
                            scale: point("scale")
                                .unwrap_or(Vec3::ONE),
                        },
                    );
                }
            }
            "line" => {
                for input in ["from", "to"] {
                    if let Some(at) = point(input) {
                        add(at, Kind::Point { input });
                    }
                }
            }
            "spline" => {
                if let Some(spline) = spline_of(editor, *node) {
                    knots(&spline, &mut add);
                }
            }
            "polygon" | "rectangle" | "circle" => {
                let middle = point("center");
                if let Some(at) = middle {
                    add(at, Kind::Point { input: "center" });
                }
                let middle = middle.unwrap_or(Vec3::ZERO);
                let sides: &[(&'static str, Vec3, f32)] =
                    if kind != "rectangle" {
                        &[("radius", Vec3::X, 1.0)]
                    } else {
                        &[
                            ("width", Vec3::X, 2.0),
                            ("depth", Vec3::Z, 2.0),
                        ]
                    };
                for &(input, axis, times) in sides {
                    if let Some(length) = number(input) {
                        add(
                            middle + axis * length / times,
                            Kind::Distance {
                                input,
                                middle,
                                axis,
                                times,
                            },
                        );
                    }
                }
            }
            _ => {}
        }
    }
    found
}

/// Gives `add` each point of `spline`, then the ends of the handles
/// the path follows when it goes by them.
fn knots(spline: &Spline, add: &mut impl FnMut(Vec3, Kind)) {
    let Spline {
        points,
        kind,
        closed,
    } = spline;
    for (index, point) in points.iter().enumerate() {
        add(
            point.at,
            Kind::Knot {
                index,
                part: Part::Anchor,
                anchor: point.at,
            },
        );
    }
    if *kind != SplineKind::Handles {
        return;
    }
    for (index, point) in points.iter().enumerate() {
        let before = *closed || index > 0;
        let after = *closed || index + 1 < points.len();
        for (part, offset, shown) in [
            (Part::Before, point.before, before),
            (Part::After, point.after, after),
        ] {
            if shown {
                add(
                    point.at + offset,
                    Kind::Knot {
                        index,
                        part,
                        anchor: point.at,
                    },
                );
            }
        }
    }
}

/// A rotation input's degrees about x, y and z as a turn, about y
/// first as the transform node reads them.
pub(super) fn rotation_of(degrees: Vec3) -> Quat {
    Quat::from_euler(
        EulerRot::YXZ,
        degrees.y.to_radians(),
        degrees.x.to_radians(),
        degrees.z.to_radians(),
    )
}

/// A turn as a rotation input's degrees about x, y and z.
pub(super) fn degrees_of(rotation: Quat) -> Vec3 {
    let (yaw, pitch, roll) = rotation.to_euler(EulerRot::YXZ);
    Vec3::new(pitch.to_degrees(), yaw.to_degrees(), roll.to_degrees())
}

/// The input of the graph that a parameter node gives to `input` of
/// `node` by a wire.
fn parameter_of<'a>(
    graph: &'a Graph,
    node: NodeId,
    input: &str,
) -> Option<&'a GraphInput> {
    let wire = graph
        .wires
        .iter()
        .find(|wire| wire.to.node == node && wire.to.name == input)?;
    let Does::Input(name) = &graph.node(wire.from.node)?.does else {
        return None;
    };
    graph
        .inputs
        .iter()
        .find(|parameter| parameter.name == *name)
}

/// The constant an input of `node` is set to: fixed, its parameter's
/// default, or its kind's initial value. None for one that is wired
/// from anything but a parameter, or set otherwise.
fn constant_of(
    editor: &Editor<Blueprint>,
    node: NodeId,
    input: &str,
) -> Option<Constant> {
    let graph = &editor.sheet.graph;
    if graph
        .wires
        .iter()
        .any(|wire| wire.to.node == node && wire.to.name == input)
    {
        return parameter_of(graph, node, input)?.default.clone();
    }
    let card = graph.node(node)?;
    match card.settings.get(input) {
        Some(Setting::Fixed(constant)) => Some(constant.clone()),
        Some(Setting::Random(_)) => None,
        None => {
            let Does::Kind(kind) = &card.does else {
                return None;
            };
            let spec = editor
                .registry
                .get(kind)?
                .inputs
                .iter()
                .find(|spec| spec.name == input)?;
            spec.initial.map(Constant::from)
        }
    }
}

fn point_of(
    editor: &Editor<Blueprint>,
    node: NodeId,
    input: &str,
) -> Option<Vec3> {
    match constant_of(editor, node, input)? {
        Constant::Point(point) => Some(point),
        _ => None,
    }
}

/// The path `input` of `node` is set to.
pub(super) fn path_of(
    editor: &Editor<Blueprint>,
    node: NodeId,
    input: &str,
) -> Option<Vec<PathPoint>> {
    match constant_of(editor, node, input)? {
        Constant::Path(path) => Some(path),
        _ => None,
    }
}

/// The spline `node` is, when it is one with nothing wired to what
/// it takes.
pub(super) fn spline_of(
    editor: &Editor<Blueprint>,
    node: NodeId,
) -> Option<Spline> {
    let Does::Kind(kind) = &editor.sheet.graph.node(node)?.does
    else {
        return None;
    };
    if kind != "spline" {
        return None;
    }
    let kind = match constant_of(editor, node, "kind")? {
        Constant::Choice(choice) => SplineKind::from_choice(&choice)?,
        _ => return None,
    };
    let Constant::Bool(closed) = constant_of(editor, node, "closed")?
    else {
        return None;
    };
    Some(Spline {
        points: path_of(editor, node, POINTS)?,
        kind,
        closed,
    })
}

fn number_of(
    editor: &Editor<Blueprint>,
    node: NodeId,
    input: &str,
) -> Option<f32> {
    match constant_of(editor, node, input)? {
        Constant::Number(number) => Some(number),
        _ => None,
    }
}

/// Sets `input` of `node` to `value`: the input itself, or the
/// default of the parameter it is wired from. Writes one after
/// another of the same input are undone as one.
pub(super) fn set(
    editor: &mut Editor<Blueprint>,
    node: NodeId,
    input: &str,
    value: Constant,
) {
    let parameter =
        parameter_of(&editor.sheet.graph, node, input).cloned();
    match parameter {
        Some(mut parameter) => {
            let name = parameter.name.clone();
            parameter.default = Some(value);
            editor.apply(Edit::ChangeInput {
                name,
                input: parameter,
            });
        }
        None => editor.apply(Edit::Set {
            node,
            input: input.to_owned(),
            setting: Some(Setting::Fixed(value)),
        }),
    }
}
