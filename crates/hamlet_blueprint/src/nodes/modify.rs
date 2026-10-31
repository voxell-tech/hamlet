//! Nodes that change a list of transforms.

use bevy_math::Vec3;

use super::{TRANSFORMS_IN, TRANSFORMS_OUT};
use crate::eval::{Problem, Run};
use crate::graph::Scope;
use crate::node::{Initial, NodeKind, PortSpec};
use crate::turn;
use crate::value::{Transform, Value, ValueType};

pub(super) const KINDS: &[NodeKind] =
    &[REPEAT, OFFSET, CULL, KEEP_APART, MERGE];

const REPEAT: NodeKind = NodeKind {
    name: "repeat",
    doc: "Each transform becomes copies, each a step further on in \
          its own frame",
    inputs: &[
        TRANSFORMS_IN,
        PortSpec::new(
            "count",
            ValueType::Integer,
            "How many copies, the first included",
        )
        .initial(Initial::Integer(2)),
        PortSpec::new(
            "step",
            ValueType::Point,
            "The offset from one copy to the next",
        )
        .initial(Initial::Point(Vec3::new(0.0, 0.0, 1.0))),
    ],
    outputs: &[TRANSFORMS_OUT],
    run: repeat,
};

fn repeat(run: &mut Run) -> Result<(), Problem> {
    let step = run.point("step")?;
    let mut placed = Vec::new();
    for original in run.transforms("transforms")? {
        let count =
            run.integer("count", Some(original))?.max(0) as u32;
        for index in 0..count {
            let mut copy = original.clone();
            copy.id = run.id(original.id, index);
            copy.frame.translation =
                original.frame.point(step * index as f32);
            placed.push(copy);
        }
    }
    run.put("transforms", Value::Transforms(placed));
    Ok(())
}

const OFFSET: NodeKind = NodeKind {
    name: "offset",
    doc: "Moves, turns and scales each transform. Random or field \
          values make it a jitter",
    inputs: &[
        TRANSFORMS_IN,
        PortSpec::new("move_x", ValueType::Number, "Metres along x")
            .initial(Initial::Number(0.0)),
        PortSpec::new("move_y", ValueType::Number, "Metres along y")
            .initial(Initial::Number(0.0)),
        PortSpec::new("move_z", ValueType::Number, "Metres along z")
            .initial(Initial::Number(0.0)),
        PortSpec::new("yaw", ValueType::Number, "Degrees about y")
            .initial(Initial::Number(0.0)),
        PortSpec::new("pitch", ValueType::Number, "Degrees about x")
            .initial(Initial::Number(0.0)),
        PortSpec::new("roll", ValueType::Number, "Degrees about z")
            .initial(Initial::Number(0.0)),
        PortSpec::new("scale", ValueType::Number, "Times its size")
            .initial(Initial::Number(1.0)),
        PortSpec::new(
            "space",
            ValueType::Choice,
            "Whether moves follow each transform's turn",
        )
        .initial(Initial::Choice("local"))
        .choices(&["local", "world"]),
    ],
    outputs: &[TRANSFORMS_OUT],
    run: offset,
};

fn offset(run: &mut Run) -> Result<(), Problem> {
    let local = run.choice("space", None)? == "local";
    let mut placed = Vec::new();
    for original in run.transforms("transforms")? {
        let at = Some(original);
        let mut moved = original.clone();
        let offset = Vec3::new(
            run.number("move_x", at)?,
            run.number("move_y", at)?,
            run.number("move_z", at)?,
        );
        moved.frame.translation += if local {
            original.frame.rotation * offset
        } else {
            offset
        };
        moved.frame.rotation = original.frame.rotation
            * turn::euler(
                run.number("yaw", at)?,
                run.number("pitch", at)?,
                run.number("roll", at)?,
            );
        moved.frame.scale *= run.number("scale", at)?;
        placed.push(moved);
    }
    run.put("transforms", Value::Transforms(placed));
    Ok(())
}

const CULL: NodeKind = NodeKind {
    name: "cull",
    doc: "Keeps the transforms whose keep is true. A random chance \
          or a field's threshold thins them",
    inputs: &[
        TRANSFORMS_IN,
        PortSpec::new(
            "keep",
            ValueType::Bool,
            "Whether each transform stays",
        )
        .initial(Initial::Bool(true)),
    ],
    outputs: &[TRANSFORMS_OUT],
    run: cull,
};

fn cull(run: &mut Run) -> Result<(), Problem> {
    let mut placed = Vec::new();
    for transform in run.transforms("transforms")? {
        if run.flag("keep", Some(transform))? {
            placed.push(transform.clone());
        }
    }
    run.put("transforms", Value::Transforms(placed));
    Ok(())
}

const KEEP_APART: NodeKind = NodeKind {
    name: "keep_apart",
    doc: "Removes transforms nearer than a distance to one kept, \
          kept in a seeded order",
    inputs: &[
        TRANSFORMS_IN,
        PortSpec::new(
            "distance",
            ValueType::Number,
            "The least metres between two kept",
        )
        .initial(Initial::Number(1.0)),
    ],
    outputs: &[TRANSFORMS_OUT],
    run: keep_apart,
};

fn keep_apart(run: &mut Run) -> Result<(), Problem> {
    let distance = run.number("distance", None)?;
    let chance = run.chance(Scope::House);
    let mut order =
        run.transforms("transforms")?.iter().collect::<Vec<_>>();
    order.sort_by_key(|transform| chance.mix(transform.id.0));
    let mut kept: Vec<&Transform> = Vec::with_capacity(order.len());
    for transform in order {
        let near = kept.iter().any(|other| {
            other
                .frame
                .translation
                .distance(transform.frame.translation)
                < distance
        });
        if !near {
            kept.push(transform);
        }
    }
    kept.sort_by_key(|transform| (transform.index, transform.id));
    let kept = kept.into_iter().cloned().collect();
    run.put("transforms", Value::Transforms(kept));
    Ok(())
}

const MERGE: NodeKind = NodeKind {
    name: "merge",
    doc: "Two lists of transforms as one, the first before the \
          second",
    inputs: &[
        PortSpec::new(
            "first",
            ValueType::Transforms,
            "The first list",
        ),
        PortSpec::new(
            "second",
            ValueType::Transforms,
            "The second list",
        ),
    ],
    outputs: &[TRANSFORMS_OUT],
    run: merge,
};

fn merge(run: &mut Run) -> Result<(), Problem> {
    let mut placed = run.transforms("first")?.to_vec();
    placed.extend_from_slice(run.transforms("second")?);
    run.put("transforms", Value::Transforms(placed));
    Ok(())
}
