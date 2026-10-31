//! Nodes that put assets down.

use bevy_math::Vec3;

use super::SPOTS_IN;
use crate::eval::{Problem, Run};
use crate::node::{Initial, Listed, NodeKind, PortSpec};
use crate::turn;
use crate::value::{Frame, Instance, Value, ValueType};

pub(super) const KINDS: &[NodeKind] = &[PART, PUT, PLACE, JOIN];

const GEOMETRY_OUT: PortSpec = PortSpec::new(
    "geometry",
    ValueType::Geometry,
    "The assets put down",
);

const PUT: NodeKind = NodeKind {
    name: "put",
    doc: "An asset at each spot, sized to it",
    inputs: &[
        SPOTS_IN,
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
            "Its size, times the spot's",
        )
        .initial(Initial::Point(Vec3::ONE)),
    ],
    outputs: &[GEOMETRY_OUT],
    run: put,
};

fn put(run: &mut Run) -> Result<(), Problem> {
    let size = run.point("size")?;
    let mut instances = Vec::new();
    for spot in run.spots("spots")? {
        instances.push(Instance {
            asset: run.choice("asset", Some(spot))?,
            frame: spot.frame,
            size: spot.size * size,
        });
    }
    run.put("geometry", Value::Geometry(instances));
    Ok(())
}

const PART: NodeKind = NodeKind {
    name: "part",
    doc: "One asset where it is put, as big as it is given and \
          turned, or one at each spot it is put on",
    inputs: &[
        PortSpec::new(
            "asset",
            ValueType::Choice,
            "The asset's name in the catalogue",
        )
        .listed(Listed::Assets)
        .initial(Initial::Choice("box")),
        PortSpec::new("at", ValueType::Point, "Where its anchor is")
            .initial(Initial::Point(Vec3::ZERO)),
        PortSpec::new(
            "anchor",
            ValueType::Choice,
            "Which point of it is at: the middle of its base, its \
             middle, or the middle of its top",
        )
        .initial(Initial::Choice("base"))
        .choices(&["base", "middle", "top"]),
        PortSpec::new(
            "size",
            ValueType::Point,
            "Its size on each axis",
        )
        .initial(Initial::Point(Vec3::ONE)),
        PortSpec::new("yaw", ValueType::Number, "Degrees about y")
            .initial(Initial::Number(0.0)),
        PortSpec::new("pitch", ValueType::Number, "Degrees about x")
            .initial(Initial::Number(0.0)),
        PortSpec::new("roll", ValueType::Number, "Degrees about z")
            .initial(Initial::Number(0.0)),
        PortSpec::new(
            "on",
            ValueType::Spots,
            "Spots to put one on each of, where it is in each \
             spot's own frame. Left unwired, there is one",
        ),
    ],
    outputs: &[GEOMETRY_OUT],
    run: part,
};

fn part(run: &mut Run) -> Result<(), Problem> {
    let asset = run.choice("asset", None)?;
    let size = run.point("size")?;
    let rotation = turn::euler(
        run.number("yaw", None)?,
        run.number("pitch", None)?,
        run.number("roll", None)?,
    );
    // The asset stands on its frame; an anchor above its base moves
    // the frame down, turned with it.
    let lift = match run.choice("anchor", None)?.as_str() {
        "middle" => 0.5,
        "top" => 1.0,
        _ => 0.0,
    };
    let own = Frame {
        translation: run.point("at")?
            - rotation * Vec3::new(0.0, size.y * lift, 0.0),
        rotation,
        scale: Vec3::ONE,
    };
    let frames = if run.is_set("on") {
        run.spots("on")?
            .iter()
            .map(|spot| Frame {
                translation: spot.frame.point(own.translation),
                rotation: spot.frame.rotation * own.rotation,
                scale: spot.frame.scale,
            })
            .collect()
    } else {
        vec![own]
    };
    let instances = frames
        .into_iter()
        .map(|frame| Instance {
            asset: asset.clone(),
            frame,
            size,
        })
        .collect();
    run.put("geometry", Value::Geometry(instances));
    Ok(())
}

const PLACE: NodeKind = NodeKind {
    name: "place",
    doc: "A copy of the assets at each spot, moved, turned and \
          scaled with it, such as a window graph at every window",
    inputs: &[
        PortSpec::new(
            "geometry",
            ValueType::Geometry,
            "The assets, about their own origin",
        ),
        SPOTS_IN,
    ],
    outputs: &[GEOMETRY_OUT],
    run: place,
};

fn place(run: &mut Run) -> Result<(), Problem> {
    let shape = run.geometry("geometry")?;
    let mut instances = Vec::new();
    for spot in run.spots("spots")? {
        let at = spot.frame;
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
