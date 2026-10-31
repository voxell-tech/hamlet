//! Nodes that make transforms.

use bevy_math::Vec3;

use super::{TRANSFORMS_IN, TRANSFORMS_OUT};
use crate::eval::{Problem, Run};
use crate::graph::Scope;
use crate::node::{Initial, Listed, NodeKind, PortSpec};
use crate::turn;
use crate::value::{Frame, Transform, TransformId, Value, ValueType};

pub(super) const KINDS: &[NodeKind] = &[
    TRANSFORM,
    GRID,
    ALONG_CURVE,
    CURVE_CORNERS,
    ON_SURFACE,
    SPLIT,
    ON_SOCKETS,
];

const CURVE_IN: PortSpec =
    PortSpec::new("curve", ValueType::Curve, "The curve to follow");
const ALIGN: PortSpec = PortSpec::new(
    "align",
    ValueType::Bool,
    "Whether each transform turns to point along the curve",
)
.initial(Initial::Bool(true));

const TRANSFORM: NodeKind = NodeKind {
    name: "transform",
    doc: "Where something stands, how it is turned, and its scale, \
          which is its size",
    inputs: &[
        PortSpec::new(
            "translation",
            ValueType::Point,
            "Where it stands: the middle of its base",
        )
        .initial(Initial::Point(Vec3::ZERO)),
        PortSpec::new(
            "rotation",
            ValueType::Point,
            "Degrees about x, y and z, turned about y first",
        )
        .initial(Initial::Point(Vec3::ZERO)),
        PortSpec::new(
            "scale",
            ValueType::Point,
            "Its size on each axis",
        )
        .initial(Initial::Point(Vec3::ONE)),
        PortSpec::new(
            "for",
            ValueType::Transforms,
            "A list to make one for each of, its inputs read at \
             each, so noise or random make each its own. Left \
             unwired, there is one",
        ),
    ],
    outputs: &[TRANSFORMS_OUT],
    run: transform,
};

const ON_SOCKETS: NodeKind = NodeKind {
    name: "on_sockets",
    doc: "A transform at a socket of each asset that has it, turned \
          as the socket is",
    inputs: &[
        PortSpec::new(
            "objects",
            ValueType::Objects,
            "The assets whose sockets to place on",
        ),
        PortSpec::new(
            "socket",
            ValueType::Choice,
            "The socket's name, as its asset's file has it",
        )
        .listed(Listed::Sockets)
        .initial(Initial::Choice("lamp")),
    ],
    outputs: &[TRANSFORMS_OUT],
    run: on_sockets,
};

fn on_sockets(run: &mut Run) -> Result<(), Problem> {
    let name = run.choice("socket", None)?;
    let mut transforms = Vec::new();
    for object in run.objects("objects")? {
        let Some(socket) = run
            .asset(&object.asset)
            .and_then(|asset| asset.sockets.get(&name).copied())
        else {
            continue;
        };
        // An asset stands on its frame, its box above it.
        let size = object.frame.scale;
        let rotation = object.frame.rotation;
        let frame = Frame {
            translation: object.frame.translation
                + rotation * ((socket.at + Vec3::Y * 0.5) * size),
            rotation: rotation * socket.turn,
            scale: Vec3::ONE,
        };
        let index = transforms.len() as u32;
        let mut transform =
            Transform::new(run.id(TransformId::ROOT, index), frame);
        transform.index = index;
        transforms.push(transform);
    }
    let count = transforms.len() as u32;
    for transform in &mut transforms {
        transform.count = count;
    }
    run.put("transforms", Value::Transforms(transforms));
    Ok(())
}

fn transform(run: &mut Run) -> Result<(), Problem> {
    let frame = |at: Option<&Transform>| -> Result<Frame, Problem> {
        let turn = run.point_at("rotation", at)?;
        Ok(Frame {
            translation: run.point_at("translation", at)?,
            rotation: turn::euler(turn.y, turn.x, turn.z),
            scale: run.point_at("scale", at)?,
        })
    };
    let made = if run.is_set("for") {
        // Each keeps the item it was made for, so matmul pairs them.
        run.transforms("for")?
            .iter()
            .map(|item| {
                let mut made = item.clone();
                made.frame = frame(Some(item))?;
                Ok(made)
            })
            .collect::<Result<Vec<_>, Problem>>()?
    } else {
        vec![Transform::new(
            run.id(TransformId::ROOT, 0),
            frame(None)?,
        )]
    };
    run.put("transforms", Value::Transforms(made));
    Ok(())
}

const GRID: NodeKind = NodeKind {
    name: "grid",
    doc: "Transforms in rows and columns on the ground, about a \
          middle",
    inputs: &[
        PortSpec::new("at", ValueType::Point, "Its middle")
            .initial(Initial::Point(Vec3::ZERO)),
        PortSpec::new(
            "columns",
            ValueType::Integer,
            "How many along x",
        )
        .initial(Initial::Integer(3)),
        PortSpec::new("rows", ValueType::Integer, "How many along z")
            .initial(Initial::Integer(3)),
        PortSpec::new(
            "spacing",
            ValueType::Number,
            "Metres between neighbours",
        )
        .initial(Initial::Number(2.0)),
    ],
    outputs: &[TRANSFORMS_OUT],
    run: grid,
};

fn grid(run: &mut Run) -> Result<(), Problem> {
    let at = run.point("at")?;
    let columns = run.integer("columns", None)?.max(0) as u32;
    let rows = run.integer("rows", None)?.max(0) as u32;
    let spacing = run.number("spacing", None)?;
    let corner = at
        - Vec3::new(
            (columns.max(1) - 1) as f32,
            0.0,
            (rows.max(1) - 1) as f32,
        ) * spacing
            * 0.5;
    let count = columns * rows;
    let mut placed = Vec::with_capacity(count as usize);
    for row in 0..rows {
        for column in 0..columns {
            let index = row * columns + column;
            let offset =
                Vec3::new(column as f32, 0.0, row as f32) * spacing;
            let mut transform = Transform::new(
                // Keyed by row and column, so adding columns keeps
                // the rest.
                run.id(TransformId(u64::from(row)), column),
                Frame::at(corner + offset),
            );
            transform.index = index;
            transform.count = count;
            placed.push(transform);
        }
    }
    run.put("transforms", Value::Transforms(placed));
    Ok(())
}

const ALONG_CURVE: NodeKind = NodeKind {
    name: "along_curve",
    doc: "Transforms along a curve, one every spacing from its start",
    inputs: &[
        CURVE_IN,
        PortSpec::new(
            "spacing",
            ValueType::Number,
            "Metres between transforms",
        )
        .initial(Initial::Number(1.0)),
        ALIGN,
    ],
    outputs: &[TRANSFORMS_OUT],
    run: along_curve,
};

fn along_curve(run: &mut Run) -> Result<(), Problem> {
    let spacing = run.number("spacing", None)?;
    if spacing <= 0.0 {
        return Err(run.invalid("spacing", "must be above zero"));
    }
    let align = run.flag("align", None)?;
    let curve = run.curve("curve")?;
    let length = curve.length();
    // Spacing is kept exact, not stretched to fit, so a longer curve
    // keeps the transforms a shorter one had.
    let steps = (length / spacing + 1e-4).floor() as u32;
    let count = if curve.closed { steps } else { steps + 1 };
    let mut placed = Vec::with_capacity(count as usize);
    for index in 0..count {
        let distance = index as f32 * spacing;
        let Some((at, direction)) = curve.at(distance) else {
            break;
        };
        let mut frame = Frame::at(at);
        if align {
            frame.rotation = turn::yaw_toward(direction);
        }
        let mut transform =
            Transform::new(run.id(TransformId::ROOT, index), frame);
        transform.index = index;
        transform.count = count;
        transform.along = distance / length;
        placed.push(transform);
    }
    run.put("transforms", Value::Transforms(placed));
    Ok(())
}

const CURVE_CORNERS: NodeKind = NodeKind {
    name: "curve_corners",
    doc: "A transform at each point of a curve",
    inputs: &[CURVE_IN, ALIGN],
    outputs: &[TRANSFORMS_OUT],
    run: curve_corners,
};

fn curve_corners(run: &mut Run) -> Result<(), Problem> {
    let align = run.flag("align", None)?;
    let curve = run.curve("curve")?;
    let count = curve.points.len() as u32;
    let length = curve.length();
    let mut travelled = 0.0;
    let mut placed = Vec::with_capacity(curve.points.len());
    for (index, &point) in curve.points.iter().enumerate() {
        let next = curve
            .points
            .get(index + 1)
            .or(curve.closed.then_some(&curve.points[0]));
        let previous = index.checked_sub(1).map(|i| curve.points[i]);
        let direction = match (previous, next) {
            (_, Some(&next)) => next - point,
            (Some(previous), None) => point - previous,
            (None, None) => Vec3::X,
        };
        let mut frame = Frame::at(point);
        if align {
            frame.rotation = turn::yaw_toward(direction);
        }
        let index = index as u32;
        let mut transform =
            Transform::new(run.id(TransformId::ROOT, index), frame);
        transform.index = index;
        transform.count = count;
        transform.along = if length > 0.0 {
            travelled / length
        } else {
            0.0
        };
        travelled += next.map_or(0.0, |&next| point.distance(next));
        placed.push(transform);
    }
    run.put("transforms", Value::Transforms(placed));
    Ok(())
}

const ON_SURFACE: NodeKind = NodeKind {
    name: "on_surface",
    doc: "Transforms at random on the top face of each transform, \
          as many as its area times the density",
    inputs: &[
        TRANSFORMS_IN,
        PortSpec::new(
            "density",
            ValueType::Number,
            "Transforms per square metre",
        )
        .initial(Initial::Number(1.0)),
        PortSpec::new(
            "align",
            ValueType::Bool,
            "Whether each turns with the face it is on",
        )
        .initial(Initial::Bool(true)),
    ],
    outputs: &[TRANSFORMS_OUT],
    run: on_surface,
};

fn on_surface(run: &mut Run) -> Result<(), Problem> {
    let align = run.flag("align", None)?;
    let chance = run.chance(Scope::Item);
    let mut placed = Vec::new();
    for surface in run.transforms("transforms")? {
        let density = run.number("density", Some(surface))?.max(0.0);
        let size = surface.frame.scale;
        let expected = size.x * size.z * density;
        let key = chance.mix(surface.id.0);
        let count = expected.floor() as u32
            + u32::from(key.unit() < expected.fract());
        for index in 0..count {
            let point = key.mix(u64::from(index) + 1);
            // On its top, in its own frame, where its scale is its
            // size.
            let local = Vec3::new(
                point.mix(1).unit() - 0.5,
                1.0,
                point.mix(2).unit() - 0.5,
            );
            let mut frame = Frame::at(surface.frame.point(local));
            if align {
                frame.rotation = surface.frame.rotation;
            }
            let mut transform =
                Transform::new(run.id(surface.id, index), frame);
            transform.index = index;
            transform.count = count;
            transform.tags.clone_from(&surface.tags);
            placed.push(transform);
        }
    }
    run.put("transforms", Value::Transforms(placed));
    Ok(())
}

const SPLIT: NodeKind = NodeKind {
    name: "split",
    doc: "Cuts each transform along an axis into equal pieces of \
          about a size",
    inputs: &[
        TRANSFORMS_IN,
        PortSpec::new(
            "axis",
            ValueType::Choice,
            "The axis to cut along, in the transform's frame",
        )
        .initial(Initial::Choice("x"))
        .choices(&["x", "y", "z"]),
        PortSpec::new(
            "size",
            ValueType::Number,
            "About how long each piece is",
        )
        .initial(Initial::Number(1.2)),
    ],
    outputs: &[TRANSFORMS_OUT],
    run: split,
};

fn split(run: &mut Run) -> Result<(), Problem> {
    let axis = match run.choice("axis", None)?.as_str() {
        "x" => Vec3::X,
        "y" => Vec3::Y,
        _ => Vec3::Z,
    };
    let mut placed = Vec::new();
    for whole in run.transforms("transforms")? {
        let size = run.number("size", Some(whole))?;
        if size <= 0.0 {
            return Err(run.invalid("size", "must be above zero"));
        }
        let length = whole.frame.scale.dot(axis);
        let count = (length / size).round().max(1.0) as u32;
        for index in 0..count {
            // In its own frame, where its scale is its size, a piece
            // is a share of one along the axis.
            let offset =
                axis * (-0.5 + (index as f32 + 0.5) / count as f32);
            let mut transform = whole.clone();
            transform.id = run.id(whole.id, index);
            transform.frame.translation = whole.frame.point(offset);
            transform.frame.scale = whole.frame.scale
                - axis * (length - length / count as f32);
            transform.index = index;
            transform.count = count;
            placed.push(transform);
        }
    }
    run.put("transforms", Value::Transforms(placed));
    Ok(())
}
