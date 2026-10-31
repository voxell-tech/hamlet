//! Nodes that make fields.

use crate::eval::{Problem, Run};
use crate::field::{Field, Measure, Noise, NoiseKind, Random};
use crate::graph::Scope;
use crate::node::{Initial, NodeKind, PortSpec};
use crate::value::{Value, ValueType};

pub(super) const KINDS: &[NodeKind] =
    &[NOISE, RANDOM, MEASURE, REMAP];

const FIELD_OUT: PortSpec =
    PortSpec::new("field", ValueType::Field, "The field");

const NOISE: NodeKind = NodeKind {
    name: "noise",
    doc: "Noise from 0 to 1, read where each placement is in the \
          world",
    inputs: &[
        PortSpec::new("kind", ValueType::Choice, "Its look")
            .initial(Initial::Choice("gradient"))
            .choices(&[
                "value",
                "gradient",
                "simplex",
                "voronoi_distance",
                "voronoi_edges",
                "voronoi_cell",
            ]),
        PortSpec::new(
            "frequency",
            ValueType::Number,
            "Features per metre",
        )
        .initial(Initial::Number(0.2)),
        PortSpec::new(
            "octaves",
            ValueType::Integer,
            "Layers of finer detail, from 1 to 8",
        )
        .initial(Initial::Integer(1)),
        PortSpec::new(
            "scope",
            ValueType::Choice,
            "How often its pattern changes",
        )
        .initial(Initial::Choice("realm"))
        .choices(&["realm", "hamlet", "house"]),
    ],
    outputs: &[FIELD_OUT],
    run: noise,
};

fn noise(run: &mut Run) -> Result<(), Problem> {
    let kind = match run.choice("kind", None)?.as_str() {
        "value" => NoiseKind::Value,
        "simplex" => NoiseKind::Simplex,
        "voronoi_distance" => NoiseKind::VoronoiDistance,
        "voronoi_edges" => NoiseKind::VoronoiEdges,
        "voronoi_cell" => NoiseKind::VoronoiCell,
        _ => NoiseKind::Gradient,
    };
    let scope = match run.choice("scope", None)?.as_str() {
        "hamlet" => Scope::Hamlet,
        "house" => Scope::House,
        _ => Scope::Realm,
    };
    let noise = Noise {
        kind,
        frequency: run.number("frequency", None)?,
        octaves: run.integer("octaves", None)?.clamp(1, 8) as u8,
        seed: run.chance(scope),
    };
    run.put("field", Value::Field(Field::Noise(noise)));
    Ok(())
}

const MEASURE: NodeKind = NodeKind {
    name: "measure",
    doc: "A fact of each placement as a field",
    inputs: &[PortSpec::new("of", ValueType::Choice, "Which fact")
        .initial(Initial::Choice("height"))
        .choices(&["x", "height", "z", "along", "index"])],
    outputs: &[FIELD_OUT],
    run: measure,
};

fn measure(run: &mut Run) -> Result<(), Problem> {
    let measure = match run.choice("of", None)?.as_str() {
        "x" => Measure::X,
        "z" => Measure::Z,
        "along" => Measure::Along,
        "index" => Measure::Index,
        _ => Measure::Height,
    };
    run.put("field", Value::Field(Field::Measure(measure)));
    Ok(())
}

const REMAP: NodeKind = NodeKind {
    name: "remap",
    doc: "A field moved from one range to another, in a straight \
          line",
    inputs: &[
        PortSpec::new(
            "field",
            ValueType::Field,
            "The field to remap",
        ),
        PortSpec::new(
            "from_low",
            ValueType::Number,
            "Becomes to_low",
        )
        .initial(Initial::Number(0.0)),
        PortSpec::new(
            "from_high",
            ValueType::Number,
            "Becomes to_high",
        )
        .initial(Initial::Number(1.0)),
        PortSpec::new(
            "to_low",
            ValueType::Number,
            "What from_low becomes",
        )
        .initial(Initial::Number(0.0)),
        PortSpec::new(
            "to_high",
            ValueType::Number,
            "What from_high becomes",
        )
        .initial(Initial::Number(1.0)),
    ],
    outputs: &[FIELD_OUT],
    run: remap,
};

fn remap(run: &mut Run) -> Result<(), Problem> {
    let field = Field::Remap {
        field: Box::new(run.field("field")?),
        from: [
            run.number("from_low", None)?,
            run.number("from_high", None)?,
        ],
        to: [
            run.number("to_low", None)?,
            run.number("to_high", None)?,
        ],
    };
    run.put("field", Value::Field(field));
    Ok(())
}

const RANDOM: NodeKind = NodeKind {
    name: "random",
    doc: "A number picked at random between low and high, again for \
          each placement, or once for each house, hamlet or realm",
    inputs: &[
        PortSpec::new(
            "low",
            ValueType::Number,
            "The least it can be",
        )
        .initial(Initial::Number(0.0)),
        PortSpec::new(
            "high",
            ValueType::Number,
            "The most it can be",
        )
        .initial(Initial::Number(1.0)),
        PortSpec::new(
            "per",
            ValueType::Choice,
            "How often it is picked again",
        )
        .initial(Initial::Choice("placement"))
        .choices(&["placement", "house", "hamlet", "realm"]),
    ],
    outputs: &[FIELD_OUT],
    run: random,
};

fn random(run: &mut Run) -> Result<(), Problem> {
    let (scope, each) = match run.choice("per", None)?.as_str() {
        "house" => (Scope::House, false),
        "hamlet" => (Scope::Hamlet, false),
        "realm" => (Scope::Realm, false),
        _ => (Scope::Placement, true),
    };
    let random = Random {
        low: run.number("low", None)?,
        high: run.number("high", None)?,
        seed: run.chance(scope),
        each,
    };
    run.put("field", Value::Field(Field::Random(random)));
    Ok(())
}
