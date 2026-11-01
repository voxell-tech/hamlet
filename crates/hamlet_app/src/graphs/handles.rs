//! What the selected cards offer the gizmo: a transform node's
//! translation, rotation and scale; a curve's ends or middle to move;
//! and a polygon's radius or a rectangle's sides to pull. An input
//! set from a parameter moves the parameter's default; a wired one
//! offers nothing.

use bevy::math::{EulerRot, Quat, Vec3};
use hamlet_graph::{Constant, Does, Edit, NodeId, Setting};
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
}

/// What every selected card offers, in the order they are selected.
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
            "polygon" | "rectangle" => {
                let middle = point("center");
                if let Some(at) = middle {
                    add(at, Kind::Point { input: "center" });
                }
                let middle = middle.unwrap_or(Vec3::ZERO);
                let sides: &[(&'static str, Vec3, f32)] =
                    if kind == "polygon" {
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

/// What an input of `node` is set to, as a constant: fixed, its
/// parameter's default, or its kind's initial value. None for one
/// that is wired, or set otherwise.
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
        return None;
    }
    let card = graph.node(node)?;
    match card.settings.get(input) {
        Some(Setting::Fixed(constant)) => Some(constant.clone()),
        Some(Setting::FromParent(name)) => graph
            .inputs
            .iter()
            .find(|parameter| parameter.name == *name)
            .and_then(|parameter| parameter.default.clone()),
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
/// default of the parameter it is set from. Writes one after another
/// of the same input are undone as one.
pub(super) fn set(
    editor: &mut Editor<Blueprint>,
    node: NodeId,
    input: &str,
    value: Constant,
) {
    let from = editor
        .sheet
        .graph
        .node(node)
        .and_then(|card| card.settings.get(input))
        .and_then(|setting| match setting {
            Setting::FromParent(name) => Some(name.clone()),
            _ => None,
        });
    let parameter = from.and_then(|name| {
        editor
            .sheet
            .graph
            .inputs
            .iter()
            .find(|parameter| parameter.name == name)
            .cloned()
    });
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
