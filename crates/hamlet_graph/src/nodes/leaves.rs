//! Nodes that put assets down.

use bevy_math::Vec3;

use super::PLACEMENTS_IN;
use crate::eval::{Problem, Run};
use crate::node::{Initial, NodeKind, PortSpec};
use crate::value::{Instance, Value, ValueType};

pub(super) const KINDS: &[NodeKind] = &[INSTANCE, JOIN];

const GEOMETRY_OUT: PortSpec = PortSpec::new(
    "geometry",
    ValueType::Geometry,
    "The assets put down",
);

const INSTANCE: NodeKind = NodeKind {
    name: "instance",
    doc: "An asset at each placement, sized to it",
    inputs: &[
        PLACEMENTS_IN,
        PortSpec::new(
            "asset",
            ValueType::Choice,
            "The asset's name in the catalogue",
        )
        .initial(Initial::Choice("box")),
        PortSpec::new(
            "size",
            ValueType::Point,
            "Its size, times the placement's",
        )
        .initial(Initial::Point(Vec3::ONE)),
    ],
    outputs: &[GEOMETRY_OUT],
    run: instance,
};

fn instance(run: &mut Run) -> Result<(), Problem> {
    let size = run.point("size")?;
    let mut instances = Vec::new();
    for placement in run.placements("placements")? {
        instances.push(Instance {
            asset: run.choice("asset", Some(placement))?,
            frame: placement.frame,
            size: placement.size * size,
        });
    }
    run.put("geometry", Value::Geometry(instances));
    Ok(())
}

const JOIN: NodeKind = NodeKind {
    name: "join",
    doc: "Two sets of assets as one",
    inputs: &[
        PortSpec::new("first", ValueType::Geometry, "The first set"),
        PortSpec::new(
            "second",
            ValueType::Geometry,
            "The second set",
        ),
    ],
    outputs: &[GEOMETRY_OUT],
    run: join,
};

fn join(run: &mut Run) -> Result<(), Problem> {
    let mut instances = run.geometry("first")?.to_vec();
    instances.extend_from_slice(run.geometry("second")?);
    run.put("geometry", Value::Geometry(instances));
    Ok(())
}
