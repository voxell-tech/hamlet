//! Nodes that make a transform, put an asset down as an object on
//! one, move objects by one, and join sets of objects.

use bevy_math::Vec3;
use hamlet_graph::{
    Error, Initial, Listed, NodeKind, PortSpec, Scope, ValueType,
};

use super::{OBJECTS_IN, OBJECTS_OUT};
use crate::eval::Run;
use crate::kind::BlueprintKind;
use crate::value::{Frame, Object, ObjectId, TransformValue, Value};

pub(super) const KINDS: &[BlueprintKind] = &[
    BlueprintKind::new(TRANSFORM, transform),
    BlueprintKind::new(OBJECT, object),
    BlueprintKind::new(MATMUL, matmul),
    BlueprintKind::new(COMBINE, combine),
    BlueprintKind::new(RANDOM_SELECT, random_select),
];

const TRANSFORM_IN: PortSpec = PortSpec::new(
    "transform",
    ValueType::Transform,
    "Where it stands, turned and scaled. Left unwired, nothing moves",
);

const TRANSFORM: NodeKind = NodeKind {
    name: "transform",
    doc: "Where something stands, how it is turned, and its scale, \
          which is its size. An axis wired from noise or random \
          makes it its own at each object it moves",
    inputs: &[
        PortSpec::new(
            "translation",
            ValueType::Point,
            "Where it stands: the middle of its base",
        )
        .initial(Initial::Point(Vec3::ZERO)),
        PortSpec::new(
            "rotation",
            ValueType::Point,
            "Degrees about x, y and z, turned about y first",
        )
        .initial(Initial::Point(Vec3::ZERO)),
        PortSpec::new(
            "scale",
            ValueType::Point,
            "Its size on each axis",
        )
        .initial(Initial::Point(Vec3::ONE)),
    ],
    outputs: &[PortSpec::new(
        "transform",
        ValueType::Transform,
        "The transform",
    )],
};

fn transform(run: &mut Run) -> Result<(), Error> {
    let transform = TransformValue {
        translation: run.point_fields("translation")?,
        rotation: run.point_fields("rotation")?,
        scale: run.point_fields("scale")?,
    };
    run.put("transform", Value::Transform(Box::new(transform)));
    Ok(())
}

const OBJECT: NodeKind = NodeKind {
    name: "object",
    doc: "An asset on a transform, whose scale is its size: a \
          built-in box is a metre on each side",
    inputs: &[
        PortSpec::new(
            "asset",
            ValueType::Choice,
            "The asset's name in the catalogue",
        )
        .listed(Listed::Assets)
        .initial(Initial::Choice("box")),
        TRANSFORM_IN,
    ],
    outputs: &[OBJECTS_OUT],
};

fn object(run: &mut Run) -> Result<(), Error> {
    let frame = run.transform("transform")?.at(run.origin());
    let object = Object::new(
        run.choice("asset", None)?,
        run.id(ObjectId::ROOT, 0_u32),
        frame,
    );
    run.put("objects", Value::Objects(vec![object]));
    Ok(())
}

const MATMUL: NodeKind = NodeKind {
    name: "matmul",
    doc: "Each object times the transform, as matrices. About each \
          object's own base, a transform read from noise or random \
          turns each its own way; about the origin, it moves them \
          all as one",
    inputs: &[
        OBJECTS_IN,
        TRANSFORM_IN,
        PortSpec::new(
            "about",
            ValueType::Choice,
            "Whether each object turns about its own base, or all \
             of them about the origin",
        )
        .initial(Initial::Choice("each"))
        .choices(&["each", "origin"]),
    ],
    outputs: &[OBJECTS_OUT],
};

fn matmul(run: &mut Run) -> Result<(), Error> {
    let transform = run.transform("transform")?;
    let each = run.choice("about", None)? == "each";
    let objects = run
        .objects("objects")?
        .iter()
        .map(|object| {
            let by = transform.at(object);
            let mut moved = object.clone();
            moved.frame = if each {
                // In its own turned frame, but not its size: a move
                // is metres, whatever the object's scale.
                Frame {
                    translation: object.frame.translation
                        + object.frame.rotation * by.translation,
                    rotation: object.frame.rotation * by.rotation,
                    scale: object.frame.scale * by.scale,
                }
            } else {
                object.copied(&by, object.id).frame
            };
            moved
        })
        .collect();
    run.put("objects", Value::Objects(objects));
    Ok(())
}

/// A set of objects, one port for each wire.
const SET_IN: PortSpec =
    PortSpec::new("set", ValueType::Objects, "A set").grows();

const COMBINE: NodeKind = NodeKind {
    name: "combine",
    doc: "Its wired sets of objects as one, in the order of its \
          inputs",
    inputs: &[SET_IN],
    outputs: &[OBJECTS_OUT],
};

fn combine(run: &mut Run) -> Result<(), Error> {
    let mut objects = Vec::new();
    for set in run.grown("set") {
        objects.extend_from_slice(run.objects(&set)?);
    }
    run.put("objects", Value::Objects(objects));
    Ok(())
}

const RANDOM_SELECT: NodeKind = NodeKind {
    name: "random_select",
    doc: "One of its wired sets of objects, picked at random. Per \
          is how often the pick changes",
    inputs: &[
        SET_IN,
        PortSpec::new(
            "per",
            ValueType::Choice,
            "How often it changes",
        )
        .initial(Initial::Choice("item"))
        .choices(&["realm", "hamlet", "item"]),
    ],
    outputs: &[OBJECTS_OUT],
};

fn random_select(run: &mut Run) -> Result<(), Error> {
    let scope = match run.choice("per", None)?.as_str() {
        "realm" => Scope::Realm,
        "hamlet" => Scope::Hamlet,
        _ => Scope::Item,
    };
    let sets = run.grown("set");
    let picked =
        (run.chance(scope).unit() * sets.len() as f32) as usize;
    let objects =
        match sets.get(picked.min(sets.len().saturating_sub(1))) {
            Some(set) => run.objects(set)?.to_vec(),
            None => Vec::new(),
        };
    run.put("objects", Value::Objects(objects));
    Ok(())
}
