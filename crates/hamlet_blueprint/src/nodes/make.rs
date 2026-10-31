//! Nodes that make placements.

use bevy_math::Vec3;

use super::{PLACEMENTS_IN, PLACEMENTS_OUT};
use crate::eval::{Problem, Run};
use crate::graph::Scope;
use crate::node::{Initial, Listed, NodeKind, PortSpec};
use crate::turn;
use crate::value::{Frame, Placement, PlacementId, Value, ValueType};

pub(super) const KINDS: &[NodeKind] = &[
    BOX,
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
    "Whether each placement turns to point along the curve",
)
.initial(Initial::Bool(true));

const BOX: NodeKind = NodeKind {
    name: "box",
    doc: "One placement of a given size",
    inputs: &[
        PortSpec::new("at", ValueType::Point, "Its middle")
            .initial(Initial::Point(Vec3::ZERO)),
        PortSpec::new(
            "size",
            ValueType::Point,
            "Its size on each axis",
        )
        .initial(Initial::Point(Vec3::new(4.0, 3.0, 3.0))),
    ],
    outputs: &[PLACEMENTS_OUT],
    run: make_box,
};

const ON_SOCKETS: NodeKind = NodeKind {
    name: "on_sockets",
    doc: "A placement at a socket of each asset that has it, turned \
          as the socket is",
    inputs: &[
        PortSpec::new(
            "geometry",
            ValueType::Geometry,
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
    outputs: &[PLACEMENTS_OUT],
    run: on_sockets,
};

fn on_sockets(run: &mut Run) -> Result<(), Problem> {
    let name = run.choice("socket", None)?;
    let mut placements = Vec::new();
    for instance in run.geometry("geometry")? {
        let Some(socket) = run
            .asset(&instance.asset)
            .and_then(|asset| asset.sockets.get(&name).copied())
        else {
            continue;
        };
        // An asset stands on its frame, its box above it.
        let size = instance.size * instance.frame.scale;
        let rotation = instance.frame.rotation;
        let frame = Frame {
            translation: instance.frame.translation
                + rotation * ((socket.at + Vec3::Y * 0.5) * size),
            rotation: rotation * socket.turn,
            scale: Vec3::ONE,
        };
        let index = placements.len() as u32;
        let mut placement =
            Placement::new(run.id(PlacementId::ROOT, index), frame);
        placement.index = index;
        placements.push(placement);
    }
    let count = placements.len() as u32;
    for placement in &mut placements {
        placement.count = count;
    }
    run.put("placements", Value::Placements(placements));
    Ok(())
}

fn make_box(run: &mut Run) -> Result<(), Problem> {
    let mut placement = Placement::new(
        run.id(PlacementId::ROOT, 0),
        Frame::at(run.point("at")?),
    );
    placement.size = run.point("size")?;
    run.put("placements", Value::Placements(vec![placement]));
    Ok(())
}

const GRID: NodeKind = NodeKind {
    name: "grid",
    doc: "Placements in rows and columns on the ground, about a \
          middle",
    inputs: &[
        PortSpec::new("at", ValueType::Point, "Its middle")
            .initial(Initial::Point(Vec3::ZERO)),
        PortSpec::new(
            "columns",
            ValueType::Number,
            "How many along x",
        )
        .initial(Initial::Number(3.0)),
        PortSpec::new("rows", ValueType::Number, "How many along z")
            .initial(Initial::Number(3.0)),
        PortSpec::new(
            "spacing",
            ValueType::Number,
            "Metres between neighbours",
        )
        .initial(Initial::Number(2.0)),
    ],
    outputs: &[PLACEMENTS_OUT],
    run: grid,
};

fn grid(run: &mut Run) -> Result<(), Problem> {
    let at = run.point("at")?;
    let columns =
        run.number("columns", None)?.round().max(0.0) as u32;
    let rows = run.number("rows", None)?.round().max(0.0) as u32;
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
            let mut placement = Placement::new(
                // Keyed by row and column, so adding columns keeps
                // the rest.
                run.id(PlacementId(u64::from(row)), column),
                Frame::at(corner + offset),
            );
            placement.index = index;
            placement.count = count;
            placed.push(placement);
        }
    }
    run.put("placements", Value::Placements(placed));
    Ok(())
}

const ALONG_CURVE: NodeKind = NodeKind {
    name: "along_curve",
    doc: "Placements along a curve, one every spacing from its start",
    inputs: &[
        CURVE_IN,
        PortSpec::new(
            "spacing",
            ValueType::Number,
            "Metres between placements",
        )
        .initial(Initial::Number(1.0)),
        ALIGN,
    ],
    outputs: &[PLACEMENTS_OUT],
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
    // keeps the placements a shorter one had.
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
        let mut placement =
            Placement::new(run.id(PlacementId::ROOT, index), frame);
        placement.index = index;
        placement.count = count;
        placement.along = distance / length;
        placed.push(placement);
    }
    run.put("placements", Value::Placements(placed));
    Ok(())
}

const CURVE_CORNERS: NodeKind = NodeKind {
    name: "curve_corners",
    doc: "A placement at each point of a curve",
    inputs: &[CURVE_IN, ALIGN],
    outputs: &[PLACEMENTS_OUT],
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
        let mut placement =
            Placement::new(run.id(PlacementId::ROOT, index), frame);
        placement.index = index;
        placement.count = count;
        placement.along = if length > 0.0 {
            travelled / length
        } else {
            0.0
        };
        travelled += next.map_or(0.0, |&next| point.distance(next));
        placed.push(placement);
    }
    run.put("placements", Value::Placements(placed));
    Ok(())
}

const ON_SURFACE: NodeKind = NodeKind {
    name: "on_surface",
    doc: "Placements at random on the top face of each placement, \
          as many as its area times the density",
    inputs: &[
        PLACEMENTS_IN,
        PortSpec::new(
            "density",
            ValueType::Number,
            "Placements per square metre",
        )
        .initial(Initial::Number(1.0)),
        PortSpec::new(
            "align",
            ValueType::Bool,
            "Whether each turns with the face it is on",
        )
        .initial(Initial::Bool(true)),
    ],
    outputs: &[PLACEMENTS_OUT],
    run: on_surface,
};

fn on_surface(run: &mut Run) -> Result<(), Problem> {
    let align = run.flag("align", None)?;
    let chance = run.chance(Scope::House);
    let mut placed = Vec::new();
    for surface in run.placements("placements")? {
        let density = run.number("density", Some(surface))?.max(0.0);
        let size = surface.size * surface.frame.scale;
        let expected = size.x * size.z * density;
        let key = chance.mix(surface.id.0);
        let count = expected.floor() as u32
            + u32::from(key.unit() < expected.fract());
        for index in 0..count {
            let point = key.mix(u64::from(index) + 1);
            let local = Vec3::new(
                (point.mix(1).unit() - 0.5) * surface.size.x,
                surface.size.y * 0.5,
                (point.mix(2).unit() - 0.5) * surface.size.z,
            );
            let mut frame = Frame::at(surface.frame.point(local));
            if align {
                frame.rotation = surface.frame.rotation;
            }
            let mut placement =
                Placement::new(run.id(surface.id, index), frame);
            placement.index = index;
            placement.count = count;
            placement.tags.clone_from(&surface.tags);
            placed.push(placement);
        }
    }
    run.put("placements", Value::Placements(placed));
    Ok(())
}

const SPLIT: NodeKind = NodeKind {
    name: "split",
    doc: "Cuts each placement along an axis into equal pieces of \
          about a size",
    inputs: &[
        PLACEMENTS_IN,
        PortSpec::new(
            "axis",
            ValueType::Choice,
            "The axis to cut along, in the placement's frame",
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
    outputs: &[PLACEMENTS_OUT],
    run: split,
};

fn split(run: &mut Run) -> Result<(), Problem> {
    let axis = match run.choice("axis", None)?.as_str() {
        "x" => Vec3::X,
        "y" => Vec3::Y,
        _ => Vec3::Z,
    };
    let mut placed = Vec::new();
    for whole in run.placements("placements")? {
        let size = run.number("size", Some(whole))?;
        if size <= 0.0 {
            return Err(run.invalid("size", "must be above zero"));
        }
        let length = whole.size.dot(axis);
        let count = (length / size).round().max(1.0) as u32;
        let piece = length / count as f32;
        for index in 0..count {
            let offset =
                axis * (-length * 0.5 + piece * (index as f32 + 0.5));
            let mut placement = whole.clone();
            placement.id = run.id(whole.id, index);
            placement.frame.translation = whole.frame.point(offset);
            placement.size = whole.size - axis * (length - piece);
            placement.index = index;
            placement.count = count;
            placed.push(placement);
        }
    }
    run.put("placements", Value::Placements(placed));
    Ok(())
}
