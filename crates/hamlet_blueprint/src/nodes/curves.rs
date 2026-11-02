//! Nodes that make curves.

use bevy_math::Vec3;
use hamlet_graph::{
    Error, Initial, NodeKind, PathPoint, PortSpec, ValueType,
};

use crate::curve::{Arc, Curve};
use crate::eval::Run;
use crate::kind::BlueprintKind;
use crate::spline::{self, SplineKind};
use crate::turn;
use crate::value::Value;

pub(super) const KINDS: &[BlueprintKind] = &[
    BlueprintKind::new(LINE, line),
    BlueprintKind::new(RECTANGLE, rectangle),
    BlueprintKind::new(POLYGON, polygon),
    BlueprintKind::new(CIRCLE, circle),
    BlueprintKind::new(SPLINE, spline),
];

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
};

fn line(run: &mut Run) -> Result<(), Error> {
    let curve = Curve::line(run.point("from")?, run.point("to")?);
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
};

fn rectangle(run: &mut Run) -> Result<(), Error> {
    let center = run.point("center")?;
    let half = Vec3::new(
        run.number("width", None)?,
        0.0,
        run.number("depth", None)?,
    ) * 0.5;
    let corners =
        [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].map(
            |(x, z)| center + Vec3::new(half.x * x, 0.0, half.z * z),
        );
    let curve = Curve::polyline(&corners, true);
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
};

fn polygon(run: &mut Run) -> Result<(), Error> {
    let center = run.point("center")?;
    let radius = run.number("radius", None)?;
    let sides = run
        .count("sides", run.integer("sides", None)?.max(3) as f32)?;
    let points = (0..sides)
        .map(|i| {
            let turn = turn::about(
                Vec3::Y,
                std::f32::consts::TAU * i as f32 / sides as f32,
            );
            center + turn * Vec3::new(radius, 0.0, 0.0)
        })
        .collect::<Vec<_>>();
    run.put("curve", Value::Curve(Curve::polyline(&points, true)));
    Ok(())
}

const CIRCLE: NodeKind = NodeKind {
    name: "circle",
    doc: "A circle, or an arc of one, round a middle on the ground, \
          starting on the side of greater x and going anticlockwise \
          seen from above",
    inputs: &[
        CENTER,
        PortSpec::new("radius", ValueType::Number, "Its radius")
            .initial(Initial::Number(1.0)),
        PortSpec::new(
            "start",
            ValueType::Number,
            "Where it starts, in degrees round from greater x",
        )
        .initial(Initial::Number(0.0)),
        PortSpec::new(
            "sweep",
            ValueType::Number,
            "How far it goes round, in degrees; 360 makes it closed",
        )
        .initial(Initial::Number(360.0)),
    ],
    outputs: &[CURVE_OUT],
};

fn circle(run: &mut Run) -> Result<(), Error> {
    let radius = run.number("radius", None)?;
    if radius <= 0.0 {
        return Err(run.invalid("radius", "must be above zero"));
    }
    let sweep = run.number("sweep", None)?.to_radians();
    if sweep == 0.0 {
        return Err(run.invalid("sweep", "must not be zero"));
    }
    let arc = Arc {
        centre: run.point("center")?,
        radius,
        normal: Vec3::Y,
        zero: Vec3::X,
        start: run.number("start", None)?.to_radians(),
        sweep,
    };
    run.put("curve", Value::Curve(Curve::from_arc(arc)));
    Ok(())
}

/// Four points and their handles in a gentle S along x.
const S_BEND: [PathPoint; 4] = [
    PathPoint::new(
        Vec3::new(-3.0, 0.0, -1.0),
        Vec3::new(-1.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
    ),
    PathPoint::new(
        Vec3::new(-1.0, 0.0, -1.0),
        Vec3::new(-0.7, 0.0, -0.35),
        Vec3::new(0.7, 0.0, 0.35),
    ),
    PathPoint::new(
        Vec3::new(1.0, 0.0, 1.0),
        Vec3::new(-0.7, 0.0, -0.35),
        Vec3::new(0.7, 0.0, 0.35),
    ),
    PathPoint::new(
        Vec3::new(3.0, 0.0, 1.0),
        Vec3::new(-1.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
    ),
];

const SPLINE: NodeKind = NodeKind {
    name: "spline",
    doc: "A smooth curve through its points, or by the handles of \
          each; its points are edited in the preview",
    inputs: &[
        PortSpec::new(
            "points",
            ValueType::Path,
            "The points, each with a handle on either side",
        )
        .initial(Initial::Path(&S_BEND)),
        PortSpec::new(
            "kind",
            ValueType::Choice,
            "Whether it goes through its points or follows their \
             handles",
        )
        .choices(SplineKind::CHOICES)
        .initial(Initial::Choice("through")),
        PortSpec::new(
            "closed",
            ValueType::Bool,
            "Whether the last point joins the first",
        )
        .initial(Initial::Bool(false)),
    ],
    outputs: &[CURVE_OUT],
};

fn spline(run: &mut Run) -> Result<(), Error> {
    let points = run.path("points")?;
    if points.len() < 2 {
        return Err(
            run.invalid("points", "needs at least two points")
        );
    }
    let kind = SplineKind::from_choice(&run.choice("kind", None)?)
        .ok_or_else(|| {
            run.invalid("kind", "is not a kind of spline")
        })?;
    let curve =
        spline::curve(points, kind, run.flag("closed", None)?);
    run.put("curve", Value::Curve(curve));
    Ok(())
}
