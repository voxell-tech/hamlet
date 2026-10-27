//! Nodes that change a list of placements.

use bevy_math::Vec3;

use super::{PLACEMENTS_IN, PLACEMENTS_OUT};
use crate::eval::{Problem, Run};
use crate::graph::Scope;
use crate::node::{Initial, NodeKind, PortSpec};
use crate::turn;
use crate::value::{Placement, Value, ValueType};

pub(super) const KINDS: &[NodeKind] =
    &[REPEAT, TRANSFORM, CULL, KEEP_APART, MERGE];

const REPEAT: NodeKind = NodeKind {
    name: "repeat",
    doc: "Each placement becomes copies, each a step further on in \
          its own frame",
    inputs: &[
        PLACEMENTS_IN,
        PortSpec::new(
            "count",
            ValueType::Number,
            "How many copies, the first included",
        )
        .initial(Initial::Number(2.0)),
        PortSpec::new(
            "step",
            ValueType::Point,
            "The offset from one copy to the next",
        )
        .initial(Initial::Point(Vec3::new(0.0, 0.0, 1.0))),
    ],
    outputs: &[PLACEMENTS_OUT],
    run: repeat,
};

fn repeat(run: &mut Run) -> Result<(), Problem> {
    let step = run.point("step")?;
    let mut placed = Vec::new();
    for original in run.placements("placements")? {
        let count =
            run.number("count", Some(original))?.round().max(0.0)
                as u32;
        for index in 0..count {
            let mut copy = original.clone();
            copy.id = run.id(original.id, index);
            copy.frame.translation =
                original.frame.point(step * index as f32);
            placed.push(copy);
        }
    }
    run.put("placements", Value::Placements(placed));
    Ok(())
}

const TRANSFORM: NodeKind = NodeKind {
    name: "transform",
    doc: "Moves, turns and scales each placement. Random or field \
          values make it a jitter",
    inputs: &[
        PLACEMENTS_IN,
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
            "Whether moves follow each placement's turn",
        )
        .initial(Initial::Choice("local"))
        .choices(&["local", "world"]),
    ],
    outputs: &[PLACEMENTS_OUT],
    run: transform,
};

fn transform(run: &mut Run) -> Result<(), Problem> {
    let local = run.choice("space", None)? == "local";
    let mut placed = Vec::new();
    for original in run.placements("placements")? {
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
    run.put("placements", Value::Placements(placed));
    Ok(())
}

const CULL: NodeKind = NodeKind {
    name: "cull",
    doc: "Keeps the placements whose keep is true. A random chance \
          or a field's threshold thins them",
    inputs: &[
        PLACEMENTS_IN,
        PortSpec::new(
            "keep",
            ValueType::Bool,
            "Whether each placement stays",
        )
        .initial(Initial::Bool(true)),
    ],
    outputs: &[PLACEMENTS_OUT],
    run: cull,
};

fn cull(run: &mut Run) -> Result<(), Problem> {
    let mut placed = Vec::new();
    for placement in run.placements("placements")? {
        if run.flag("keep", Some(placement))? {
            placed.push(placement.clone());
        }
    }
    run.put("placements", Value::Placements(placed));
    Ok(())
}

const KEEP_APART: NodeKind = NodeKind {
    name: "keep_apart",
    doc: "Removes placements nearer than a distance to one kept, \
          kept in a seeded order",
    inputs: &[
        PLACEMENTS_IN,
        PortSpec::new(
            "distance",
            ValueType::Number,
            "The least metres between two kept",
        )
        .initial(Initial::Number(1.0)),
    ],
    outputs: &[PLACEMENTS_OUT],
    run: keep_apart,
};

fn keep_apart(run: &mut Run) -> Result<(), Problem> {
    let distance = run.number("distance", None)?;
    let chance = run.chance(Scope::House);
    let mut order =
        run.placements("placements")?.iter().collect::<Vec<_>>();
    order.sort_by_key(|placement| chance.mix(placement.id.0));
    let mut kept: Vec<&Placement> = Vec::with_capacity(order.len());
    for placement in order {
        let near = kept.iter().any(|other| {
            other
                .frame
                .translation
                .distance(placement.frame.translation)
                < distance
        });
        if !near {
            kept.push(placement);
        }
    }
    kept.sort_by_key(|placement| (placement.index, placement.id));
    let kept = kept.into_iter().cloned().collect();
    run.put("placements", Value::Placements(kept));
    Ok(())
}

const MERGE: NodeKind = NodeKind {
    name: "merge",
    doc: "Two lists of placements as one, the first before the \
          second",
    inputs: &[
        PortSpec::new(
            "first",
            ValueType::Placements,
            "The first list",
        ),
        PortSpec::new(
            "second",
            ValueType::Placements,
            "The second list",
        ),
    ],
    outputs: &[PLACEMENTS_OUT],
    run: merge,
};

fn merge(run: &mut Run) -> Result<(), Problem> {
    let mut placed = run.placements("first")?.to_vec();
    placed.extend_from_slice(run.placements("second")?);
    run.put("placements", Value::Placements(placed));
    Ok(())
}
