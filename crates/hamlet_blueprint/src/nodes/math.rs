//! Nodes that work out numbers and points, so a graph's inputs can
//! size and place what it puts down. Math works on fields, so noise
//! or a random number in it is worked out at each spot.

use bevy_math::Vec3;

use crate::eval::{Problem, Run};
use crate::field::{Field, Op};
use crate::node::{Initial, NodeKind, PortSpec};
use crate::value::{Value, ValueType};

pub(super) const KINDS: &[NodeKind] = &[MATH, POINT];

const MATH: NodeKind = NodeKind {
    name: "math",
    doc: "A number worked out from a and b, at each spot. Abs, \
          floor, ceil, round, sqrt, sin, cos and tan take a alone, \
          in degrees for the last three. Angle is the angle in \
          degrees whose rise is a over a run of b. Less and greater \
          give 1 or 0",
    inputs: &[
        PortSpec::new("op", ValueType::Choice, "What to work out")
            .initial(Initial::Choice("add"))
            .choices(&[
                "add", "subtract", "multiply", "divide", "min",
                "max", "power", "modulo", "abs", "floor", "ceil",
                "round", "sqrt", "sin", "cos", "tan", "hypot",
                "angle", "less", "greater",
            ])
            .names(),
        PortSpec::new("a", ValueType::Field, "The first number")
            .initial(Initial::Number(0.0)),
        PortSpec::new("b", ValueType::Field, "The second number")
            .initial(Initial::Number(0.0))
            .used_with(&[
                "add", "subtract", "multiply", "divide", "min",
                "max", "power", "modulo", "hypot", "angle", "less",
                "greater",
            ]),
    ],
    outputs: &[PortSpec::new(
        "value",
        ValueType::Field,
        "The result",
    )],
    run: math,
};

fn math(run: &mut Run) -> Result<(), Problem> {
    let name = run.choice("op", None)?;
    let op = Op::ALL
        .iter()
        .find(|(op_name, _)| *op_name == name)
        .map_or(Op::Add, |(_, op)| *op);
    let (a, b) = (run.field("a")?, run.field("b")?);
    // Two constants make a constant, so a graph's sizes stay plain.
    let value = match (&a, &b) {
        (Field::Constant(a), Field::Constant(b)) => {
            Field::Constant(op.apply(*a, *b))
        }
        _ => Field::Math(op, Box::new(a), Box::new(b)),
    };
    run.put("value", Value::Field(value));
    Ok(())
}

const POINT: NodeKind = NodeKind {
    name: "point",
    doc: "A point from three numbers",
    inputs: &[
        PortSpec::new("x", ValueType::Number, "Along x")
            .initial(Initial::Number(0.0)),
        PortSpec::new("y", ValueType::Number, "Along y")
            .initial(Initial::Number(0.0)),
        PortSpec::new("z", ValueType::Number, "Along z")
            .initial(Initial::Number(0.0)),
    ],
    outputs: &[PortSpec::new("point", ValueType::Point, "The point")],
    run: point,
};

fn point(run: &mut Run) -> Result<(), Problem> {
    let point = Vec3::new(
        run.number("x", None)?,
        run.number("y", None)?,
        run.number("z", None)?,
    );
    run.put("point", Value::Point(point));
    Ok(())
}
