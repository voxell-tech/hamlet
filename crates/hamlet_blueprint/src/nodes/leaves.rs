//! Nodes that put assets down.

use bevy_math::Vec3;

use super::PLACEMENTS_IN;
use crate::eval::{Problem, Run};
use crate::node::{Initial, Listed, NodeKind, PortSpec};
use crate::value::{Frame, Instance, Value, ValueType};

pub(super) const KINDS: &[NodeKind] = &[INSTANCE, PLACE, JOIN];

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
        .listed(Listed::Assets)
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

const PLACE: NodeKind = NodeKind {
    name: "place",
    doc: "A copy of the assets at each placement, moved, turned and \
          scaled with it, such as a window graph at every window",
    inputs: &[
        PortSpec::new(
            "geometry",
            ValueType::Geometry,
            "The assets, about their own origin",
        ),
        PLACEMENTS_IN,
    ],
    outputs: &[GEOMETRY_OUT],
    run: place,
};

fn place(run: &mut Run) -> Result<(), Problem> {
    let shape = run.geometry("geometry")?;
    let mut instances = Vec::new();
    for placement in run.placements("placements")? {
        let at = placement.frame;
        instances.extend(shape.iter().map(|instance| Instance {
            asset: instance.asset.clone(),
            frame: Frame {
                translation: at.point(instance.frame.translation),
                rotation: at.rotation * instance.frame.rotation,
                scale: at.scale * instance.frame.scale,
            },
            size: instance.size,
        }));
    }
    run.put("geometry", Value::Geometry(instances));
    Ok(())
}

/// The inputs of [`JOIN`], in the order their assets are listed.
const PARTS: [&str; 6] = ["a", "b", "c", "d", "e", "f"];

const JOIN: NodeKind = NodeKind {
    name: "join",
    doc: "Up to six sets of assets as one. Unwired inputs add \
          nothing",
    inputs: &[
        PortSpec::new("a", ValueType::Geometry, "A set"),
        PortSpec::new("b", ValueType::Geometry, "A set"),
        PortSpec::new("c", ValueType::Geometry, "A set"),
        PortSpec::new("d", ValueType::Geometry, "A set"),
        PortSpec::new("e", ValueType::Geometry, "A set"),
        PortSpec::new("f", ValueType::Geometry, "A set"),
    ],
    outputs: &[GEOMETRY_OUT],
    run: join,
};

fn join(run: &mut Run) -> Result<(), Problem> {
    let mut instances = Vec::new();
    for part in PARTS {
        if run.is_set(part) {
            instances.extend_from_slice(run.geometry(part)?);
        }
    }
    run.put("geometry", Value::Geometry(instances));
    Ok(())
}
