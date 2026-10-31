//! Nodes that make curves.

use bevy_math::Vec3;

use crate::eval::{Problem, Run};
use crate::node::{Initial, NodeKind, PortSpec};
use crate::turn;
use crate::value::{Curve, Value, ValueType};

pub(super) const KINDS: &[NodeKind] = &[LINE, RECTANGLE, POLYGON];

const CURVE_OUT: PortSpec =
    PortSpec::new("curve", ValueType::Curve, "The curve");
const CENTER: PortSpec =
    PortSpec::new("center", ValueType::Point, "Its middle")
        .initial(Initial::Point(Vec3::ZERO));

const LINE: NodeKind = NodeKind {
    name: "line",
    doc: "A straight curve between two points",
    inputs: &[
        PortSpec::new("from", ValueType::Point, "Where it starts")
            .initial(Initial::Point(Vec3::ZERO)),
        PortSpec::new("to", ValueType::Point, "Where it ends")
            .initial(Initial::Point(Vec3::new(4.0, 0.0, 0.0))),
    ],
    outputs: &[CURVE_OUT],
    run: line,
};

fn line(run: &mut Run) -> Result<(), Problem> {
    let curve = Curve {
        points: vec![run.point("from")?, run.point("to")?],
        closed: false,
    };
    run.put("curve", Value::Curve(curve));
    Ok(())
}

const RECTANGLE: NodeKind = NodeKind {
    name: "rectangle",
    doc: "A closed curve round a rectangle on the ground, starting \
          at its corner of least x and z",
    inputs: &[
        CENTER,
        PortSpec::new("width", ValueType::Number, "Its size along x")
            .initial(Initial::Number(6.0)),
        PortSpec::new("depth", ValueType::Number, "Its size along z")
            .initial(Initial::Number(4.0)),
    ],
    outputs: &[CURVE_OUT],
    run: rectangle,
};

fn rectangle(run: &mut Run) -> Result<(), Problem> {
    let center = run.point("center")?;
    let half = Vec3::new(
        run.number("width", None)?,
        0.0,
        run.number("depth", None)?,
    ) * 0.5;
    let curve = Curve {
        points: [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
            .map(|(x, z)| {
                center + Vec3::new(half.x * x, 0.0, half.z * z)
            })
            .to_vec(),
        closed: true,
    };
    run.put("curve", Value::Curve(curve));
    Ok(())
}

const POLYGON: NodeKind = NodeKind {
    name: "polygon",
    doc: "A closed curve of equal straight edges round a middle on \
          the ground, near a circle with many sides",
    inputs: &[
        CENTER,
        PortSpec::new("radius", ValueType::Number, "Its radius")
            .initial(Initial::Number(3.0)),
        PortSpec::new(
            "sides",
            ValueType::Integer,
            "How many edges, at least 3",
        )
        .initial(Initial::Integer(16)),
    ],
    outputs: &[CURVE_OUT],
    run: polygon,
};

fn polygon(run: &mut Run) -> Result<(), Problem> {
    let center = run.point("center")?;
    let radius = run.number("radius", None)?;
    let sides = run.integer("sides", None)?.max(3) as u32;
    let points = (0..sides)
        .map(|i| {
            let turn = turn::about(
                Vec3::Y,
                std::f32::consts::TAU * i as f32 / sides as f32,
            );
            center + turn * Vec3::new(radius, 0.0, 0.0)
        })
        .collect();
    run.put(
        "curve",
        Value::Curve(Curve {
            points,
            closed: true,
        }),
    );
    Ok(())
}
