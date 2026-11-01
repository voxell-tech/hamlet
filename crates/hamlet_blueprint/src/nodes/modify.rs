//! Nodes that thin or cut a set of objects.

use bevy_math::Vec3;
use hamlet_graph::{
    Error, Initial, NodeKind, PortSpec, Scope, ValueType,
};

use super::{OBJECTS_IN, OBJECTS_OUT};
use crate::eval::Run;
use crate::kind::BlueprintKind;
use crate::value::{Object, Value};

pub(super) const KINDS: &[BlueprintKind] = &[
    BlueprintKind::new(CULL, cull),
    BlueprintKind::new(KEEP_APART, keep_apart),
    BlueprintKind::new(SPLIT, split),
];

const CULL: NodeKind = NodeKind {
    name: "cull",
    doc: "Keeps the objects whose keep is true. A random chance or \
          a field's threshold thins them",
    inputs: &[
        OBJECTS_IN,
        PortSpec::new(
            "keep",
            ValueType::Bool,
            "Whether each object stays",
        )
        .initial(Initial::Bool(true)),
    ],
    outputs: &[OBJECTS_OUT],
};

fn cull(run: &mut Run) -> Result<(), Error> {
    let mut kept = Vec::new();
    for object in run.objects("objects")? {
        if run.flag("keep", Some(object))? {
            kept.push(object.clone());
        }
    }
    run.put("objects", Value::Objects(kept));
    Ok(())
}

const KEEP_APART: NodeKind = NodeKind {
    name: "keep_apart",
    doc: "Removes objects nearer than a distance to one kept, kept \
          in a seeded order",
    inputs: &[
        OBJECTS_IN,
        PortSpec::new(
            "distance",
            ValueType::Number,
            "The least metres between two kept",
        )
        .initial(Initial::Number(1.0)),
    ],
    outputs: &[OBJECTS_OUT],
};

fn keep_apart(run: &mut Run) -> Result<(), Error> {
    let distance = run.number("distance", None)?;
    let chance = run.chance(Scope::Item);
    let mut order =
        run.objects("objects")?.iter().collect::<Vec<_>>();
    order.sort_by_key(|object| chance.mix(object.id.0));
    let mut kept: Vec<&Object> = Vec::with_capacity(order.len());
    for object in order {
        let near = kept.iter().any(|other| {
            other.frame.translation.distance(object.frame.translation)
                < distance
        });
        if !near {
            kept.push(object);
        }
    }
    kept.sort_by_key(|object| (object.index, object.id));
    let kept = kept.into_iter().cloned().collect();
    run.put("objects", Value::Objects(kept));
    Ok(())
}

const SPLIT: NodeKind = NodeKind {
    name: "split",
    doc: "Cuts each object along an axis into equal pieces of about \
          a size, each the same asset",
    inputs: &[
        OBJECTS_IN,
        PortSpec::new(
            "axis",
            ValueType::Choice,
            "The axis to cut along, in the object's frame",
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
    outputs: &[OBJECTS_OUT],
};

fn split(run: &mut Run) -> Result<(), Error> {
    let axis = match run.choice("axis", None)?.as_str() {
        "x" => Vec3::X,
        "y" => Vec3::Y,
        _ => Vec3::Z,
    };
    let mut pieces = Vec::new();
    for whole in run.objects("objects")? {
        let size = run.number("size", Some(whole))?;
        if size <= 0.0 {
            return Err(run.invalid("size", "must be above zero"));
        }
        let length = whole.frame.scale.dot(axis);
        let count =
            run.count("size", (length / size).round().max(1.0))?;
        for index in 0..count {
            // In its own frame, where its scale is its size, a piece
            // is a share of one along the axis.
            let offset =
                axis * (-0.5 + (index as f32 + 0.5) / count as f32);
            let mut piece = whole.clone();
            piece.id = run.id(whole.id, index);
            piece.frame.translation = whole.frame.point(offset);
            piece.frame.scale = whole.frame.scale
                - axis * (length - length / count as f32);
            piece.index = index;
            piece.count = count;
            pieces.push(piece);
        }
    }
    run.put("objects", Value::Objects(pieces));
    Ok(())
}
