//! Nodes that put assets down as objects, and work on objects.

use bevy_math::Vec3;

use crate::eval::{Problem, Run};
use crate::node::{Initial, Listed, NodeKind, PortSpec};
use crate::value::{Frame, Object, Value, ValueType};

pub(super) const KINDS: &[NodeKind] =
    &[OBJECT, MOVE_OBJECTS, COMBINE];

const OBJECTS_OUT: PortSpec =
    PortSpec::new("objects", ValueType::Objects, "The objects");

const OBJECT: NodeKind = NodeKind {
    name: "object",
    doc: "An asset, at its transform, or one at each of the \
          transforms wired in. Its transform's scale is its size: a \
          built-in box is a metre on each side",
    inputs: &[
        PortSpec::new(
            "asset",
            ValueType::Choice,
            "The asset's name in the catalogue",
        )
        .listed(Listed::Assets)
        .initial(Initial::Choice("box")),
        PortSpec::new(
            "transform",
            ValueType::Transforms,
            "Where it stands, turned and scaled. Left unwired, it \
             is at the origin",
        ),
        PortSpec::new(
            "scale",
            ValueType::Point,
            "Times each transform's scale, so one size goes on many \
             transforms",
        )
        .initial(Initial::Point(Vec3::ONE)),
    ],
    outputs: &[OBJECTS_OUT],
    run: object,
};

fn object(run: &mut Run) -> Result<(), Problem> {
    let scale = run.point("scale")?;
    let mut objects = Vec::new();
    if run.is_set("transform") {
        for transform in run.transforms("transform")? {
            let mut frame = transform.frame;
            frame.scale *= scale;
            objects.push(Object {
                asset: run.choice("asset", Some(transform))?,
                frame,
            });
        }
    } else {
        objects.push(Object {
            asset: run.choice("asset", None)?,
            frame: Frame {
                scale,
                ..Frame::IDENTITY
            },
        });
    }
    run.put("objects", Value::Objects(objects));
    Ok(())
}

const MOVE_OBJECTS: NodeKind = NodeKind {
    name: "move_objects",
    doc: "The objects moved, turned and scaled as one by each \
          transform, a copy for each: a window built once, at every \
          window",
    inputs: &[
        PortSpec::new(
            "objects",
            ValueType::Objects,
            "The objects, about their own origin",
        ),
        PortSpec::new(
            "transform",
            ValueType::Transforms,
            "Where each copy goes",
        ),
    ],
    outputs: &[OBJECTS_OUT],
    run: move_objects,
};

fn move_objects(run: &mut Run) -> Result<(), Problem> {
    let shape = run.objects("objects")?;
    let mut objects = Vec::new();
    for transform in run.transforms("transform")? {
        let at = transform.frame;
        objects.extend(shape.iter().map(|object| Object {
            asset: object.asset.clone(),
            frame: Frame {
                translation: at.point(object.frame.translation),
                rotation: at.rotation * object.frame.rotation,
                scale: at.scale * object.frame.scale,
            },
        }));
    }
    run.put("objects", Value::Objects(objects));
    Ok(())
}

/// The inputs of [`COMBINE`], in the order their objects are listed.
const SETS: [&str; 6] = ["a", "b", "c", "d", "e", "f"];

const COMBINE: NodeKind = NodeKind {
    name: "combine",
    doc: "Up to six sets of objects as one. Unwired inputs add \
          nothing",
    inputs: &[
        PortSpec::new("a", ValueType::Objects, "A set"),
        PortSpec::new("b", ValueType::Objects, "A set"),
        PortSpec::new("c", ValueType::Objects, "A set"),
        PortSpec::new("d", ValueType::Objects, "A set"),
        PortSpec::new("e", ValueType::Objects, "A set"),
        PortSpec::new("f", ValueType::Objects, "A set"),
    ],
    outputs: &[OBJECTS_OUT],
    run: combine,
};

fn combine(run: &mut Run) -> Result<(), Problem> {
    let mut objects = Vec::new();
    for set in SETS {
        if run.is_set(set) {
            objects.extend_from_slice(run.objects(set)?);
        }
    }
    run.put("objects", Value::Objects(objects));
    Ok(())
}
