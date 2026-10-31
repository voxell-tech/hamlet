//! Nodes that change a list of transforms.

use bevy_math::Vec3;

use super::{TRANSFORMS_IN, TRANSFORMS_OUT};
use crate::eval::{Problem, Run};
use crate::graph::Scope;
use crate::node::{Initial, NodeKind, PortSpec};
use crate::value::{Frame, Transform, Value, ValueType};

pub(super) const KINDS: &[NodeKind] =
    &[MATMUL, REPEAT, CULL, KEEP_APART, MERGE];

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

const MATMUL: NodeKind = NodeKind {
    name: "matmul",
    doc: "Each transform of a times b, as matrices: b done in a's \
          own frame. One b goes on every a, one a takes every b, \
          and lists of the same length go pair by pair",
    inputs: &[
        PortSpec::new(
            "a",
            ValueType::Transforms,
            "The outer transforms",
        ),
        PortSpec::new(
            "b",
            ValueType::Transforms,
            "The transforms done in each a's frame",
        ),
    ],
    outputs: &[TRANSFORMS_OUT],
    run: matmul,
};

fn matmul(run: &mut Run) -> Result<(), Problem> {
    let (a, b) = (run.transforms("a")?, run.transforms("b")?);
    // Each result keeps the item of the list it was one of, so ids
    // and counts hold for what comes after.
    let times = |outer: &Transform,
                 inner: &Transform,
                 keep: &Transform| {
        let mut made = keep.clone();
        made.frame = Frame {
            translation: outer.frame.point(inner.frame.translation),
            rotation: outer.frame.rotation * inner.frame.rotation,
            scale: outer.frame.scale * inner.frame.scale,
        };
        made
    };
    let made: Vec<Transform> = match (a.len(), b.len()) {
        (_, 1) => {
            a.iter().map(|outer| times(outer, &b[0], outer)).collect()
        }
        (1, _) => {
            b.iter().map(|inner| times(&a[0], inner, inner)).collect()
        }
        (left, right) if left == right => a
            .iter()
            .zip(b)
            .map(|(outer, inner)| times(outer, inner, outer))
            .collect(),
        _ => {
            return Err(run.invalid(
                "b",
                "must be one transform, or as many as a",
            ));
        }
    };
    run.put("transforms", Value::Transforms(made));
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
    let chance = run.chance(Scope::Item);
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
