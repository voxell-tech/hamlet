//! Nodes that work out numbers and points, so a graph's inputs can
//! size and place what it puts down.

use bevy_math::Vec3;

use crate::eval::{Problem, Run};
use crate::node::{Initial, NodeKind, PortSpec};
use crate::value::{Value, ValueType};

pub(super) const KINDS: &[NodeKind] = &[MATH, POINT];

const MATH: NodeKind = NodeKind {
    name: "math",
    doc: "A number worked out from two. Round takes a alone; angle \
          is the angle in degrees whose rise is a over a run of b",
    inputs: &[
        PortSpec::new("op", ValueType::Choice, "What to work out")
            .initial(Initial::Choice("add"))
            .choices(&[
                "add", "subtract", "multiply", "divide", "min",
                "max", "round", "hypot", "angle",
            ]),
        PortSpec::new("a", ValueType::Number, "The first number")
            .initial(Initial::Number(0.0)),
        PortSpec::new("b", ValueType::Number, "The second number")
            .initial(Initial::Number(0.0)),
    ],
    outputs: &[PortSpec::new(
        "value",
        ValueType::Number,
        "The result",
    )],
    run: math,
};

fn math(run: &mut Run) -> Result<(), Problem> {
    let (a, b) = (run.number("a", None)?, run.number("b", None)?);
    let value = match run.choice("op", None)?.as_str() {
        "subtract" => a - b,
        "multiply" => a * b,
        "divide" if b == 0.0 => 0.0,
        "divide" => a / b,
        "min" => a.min(b),
        "max" => a.max(b),
        "round" => a.round(),
        // sqrt is exact on every platform, so this stays the same.
        "hypot" => (a * a + b * b).sqrt(),
        "angle" => libm::atan2f(a, b).to_degrees(),
        _ => a + b,
    };
    run.put("value", Value::Number(value));
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
