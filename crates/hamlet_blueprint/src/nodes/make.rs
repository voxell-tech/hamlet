//! Nodes that copy objects: each makes a copy of every object it is
//! given at each of its places, so a group keeps its shape, and one
//! copier after another copies the copies.

use bevy_math::Vec3;

use super::{OBJECTS_IN, OBJECTS_OUT};
use crate::eval::{Problem, Run};
use crate::graph::Scope;
use crate::node::{Initial, Listed, NodeKind, PortSpec};
use crate::turn;
use crate::value::{Frame, Object, Value, ValueType};

pub(super) const KINDS: &[NodeKind] = &[
    REPEAT,
    GRID,
    ALONG_CURVE,
    CURVE_CORNERS,
    ON_SURFACE,
    ON_SOCKETS,
];

const CURVE_IN: PortSpec =
    PortSpec::new("curve", ValueType::Curve, "The curve to follow");
const ALIGN: PortSpec = PortSpec::new(
    "align",
    ValueType::Bool,
    "Whether each copy turns to point along the curve",
)
.initial(Initial::Bool(true));

/// One place a copier puts a copy of its objects, and how far along
/// a curve it is, which fields of the copies read.
struct Place {
    frame: Frame,
    along: f32,
}

impl Place {
    fn at(frame: Frame) -> Self {
        Self { frame, along: 0.0 }
    }
}

/// A copy of every object at each place, each keyed by the object it
/// copies and its place's index, so adding places keeps the copies
/// already made.
fn copies(
    run: &Run,
    objects: &[Object],
    places: &[Place],
) -> Vec<Object> {
    let count = places.len() as u32;
    let mut made = Vec::with_capacity(objects.len() * places.len());
    for (index, place) in places.iter().enumerate() {
        let index = index as u32;
        for object in objects {
            let mut copy =
                object.copied(&place.frame, run.id(object.id, index));
            copy.index = index;
            copy.count = count;
            copy.along = place.along;
            made.push(copy);
        }
    }
    made
}

const REPEAT: NodeKind = NodeKind {
    name: "repeat",
    doc: "The objects again and again, each copy a step further on",
    inputs: &[
        OBJECTS_IN,
        PortSpec::new(
            "count",
            ValueType::Integer,
            "How many copies, the first included",
        )
        .initial(Initial::Integer(2)),
        PortSpec::new(
            "step",
            ValueType::Point,
            "The move from one copy to the next",
        )
        .initial(Initial::Point(Vec3::new(1.0, 0.0, 0.0))),
    ],
    outputs: &[OBJECTS_OUT],
    run: repeat,
};

fn repeat(run: &mut Run) -> Result<(), Problem> {
    let count = run.integer("count", None)?.max(0) as u32;
    let step = run.point("step")?;
    let places: Vec<Place> = (0..count)
        .map(|index| Place::at(Frame::at(step * index as f32)))
        .collect();
    let made = copies(run, run.objects("objects")?, &places);
    run.put("objects", Value::Objects(made));
    Ok(())
}

const GRID: NodeKind = NodeKind {
    name: "grid",
    doc: "The objects in rows and columns on the ground, about the \
          origin",
    inputs: &[
        OBJECTS_IN,
        PortSpec::new(
            "columns",
            ValueType::Integer,
            "How many along x",
        )
        .initial(Initial::Integer(3)),
        PortSpec::new("rows", ValueType::Integer, "How many along z")
            .initial(Initial::Integer(3)),
        PortSpec::new(
            "spacing",
            ValueType::Number,
            "Metres between neighbours",
        )
        .initial(Initial::Number(2.0)),
    ],
    outputs: &[OBJECTS_OUT],
    run: grid,
};

fn grid(run: &mut Run) -> Result<(), Problem> {
    let columns = run.integer("columns", None)?.max(0) as u32;
    let rows = run.integer("rows", None)?.max(0) as u32;
    let spacing = run.number("spacing", None)?;
    let corner = -Vec3::new(
        columns.saturating_sub(1) as f32,
        0.0,
        rows.saturating_sub(1) as f32,
    ) * spacing
        * 0.5;
    let places: Vec<Place> = (0..rows)
        .flat_map(|row| (0..columns).map(move |column| (row, column)))
        .map(|(row, column)| {
            let offset =
                Vec3::new(column as f32, 0.0, row as f32) * spacing;
            Place::at(Frame::at(corner + offset))
        })
        .collect();
    let made = copies(run, run.objects("objects")?, &places);
    run.put("objects", Value::Objects(made));
    Ok(())
}

const ALONG_CURVE: NodeKind = NodeKind {
    name: "along_curve",
    doc: "The objects along a curve, a copy every spacing from its \
          start",
    inputs: &[
        OBJECTS_IN,
        CURVE_IN,
        PortSpec::new(
            "spacing",
            ValueType::Number,
            "Metres between copies",
        )
        .initial(Initial::Number(1.0)),
        ALIGN,
    ],
    outputs: &[OBJECTS_OUT],
    run: along_curve,
};

fn along_curve(run: &mut Run) -> Result<(), Problem> {
    let spacing = run.number("spacing", None)?;
    if spacing <= 0.0 {
        return Err(run.invalid("spacing", "must be above zero"));
    }
    let align = run.flag("align", None)?;
    let curve = run.curve("curve")?;
    let length = curve.length();
    // Spacing is kept exact, not stretched to fit, so a longer curve
    // keeps the copies a shorter one had.
    let steps = (length / spacing + 1e-4).floor() as u32;
    let count = if curve.closed { steps } else { steps + 1 };
    let mut places = Vec::with_capacity(count as usize);
    for index in 0..count {
        let distance = index as f32 * spacing;
        let Some((at, direction)) = curve.at(distance) else {
            break;
        };
        let mut frame = Frame::at(at);
        if align {
            frame.rotation = turn::yaw_toward(direction);
        }
        places.push(Place {
            frame,
            along: distance / length,
        });
    }
    let made = copies(run, run.objects("objects")?, &places);
    run.put("objects", Value::Objects(made));
    Ok(())
}

const CURVE_CORNERS: NodeKind = NodeKind {
    name: "curve_corners",
    doc: "The objects at each point of a curve, such as each corner \
          of a polygon",
    inputs: &[OBJECTS_IN, CURVE_IN, ALIGN],
    outputs: &[OBJECTS_OUT],
    run: curve_corners,
};

fn curve_corners(run: &mut Run) -> Result<(), Problem> {
    let align = run.flag("align", None)?;
    let curve = run.curve("curve")?;
    let length = curve.length();
    let mut travelled = 0.0;
    let mut places = Vec::with_capacity(curve.points.len());
    for (index, &point) in curve.points.iter().enumerate() {
        let next = curve
            .points
            .get(index + 1)
            .or(curve.closed.then_some(&curve.points[0]));
        let previous = index.checked_sub(1).map(|i| curve.points[i]);
        let direction = match (previous, next) {
            (_, Some(&next)) => next - point,
            (Some(previous), None) => point - previous,
            (None, None) => Vec3::X,
        };
        let mut frame = Frame::at(point);
        if align {
            frame.rotation = turn::yaw_toward(direction);
        }
        places.push(Place {
            frame,
            along: if length > 0.0 {
                travelled / length
            } else {
                0.0
            },
        });
        travelled += next.map_or(0.0, |&next| point.distance(next));
    }
    let made = copies(run, run.objects("objects")?, &places);
    run.put("objects", Value::Objects(made));
    Ok(())
}

const ON_SURFACE: NodeKind = NodeKind {
    name: "on_surface",
    doc: "The objects at random on the top of each object they are \
          put on, as many as that top's area times the density",
    inputs: &[
        OBJECTS_IN,
        PortSpec::new(
            "on",
            ValueType::Objects,
            "The objects whose tops to scatter on",
        ),
        PortSpec::new(
            "density",
            ValueType::Number,
            "Copies per square metre",
        )
        .initial(Initial::Number(1.0)),
        PortSpec::new(
            "align",
            ValueType::Bool,
            "Whether each copy turns with the top it is on",
        )
        .initial(Initial::Bool(true)),
    ],
    outputs: &[OBJECTS_OUT],
    run: on_surface,
};

fn on_surface(run: &mut Run) -> Result<(), Problem> {
    let align = run.flag("align", None)?;
    let chance = run.chance(Scope::Item);
    let objects = run.objects("objects")?;
    let mut made = Vec::new();
    for surface in run.objects("on")? {
        let density = run.number("density", Some(surface))?.max(0.0);
        let size = surface.frame.scale;
        let expected = size.x * size.z * density;
        let key = chance.mix(surface.id.0);
        let count = expected.floor() as u32
            + u32::from(key.unit() < expected.fract());
        let places: Vec<Place> = (0..count)
            .map(|index| {
                let point = key.mix(u64::from(index) + 1);
                // On its top, in its own frame, where its scale is
                // its size.
                let local = Vec3::new(
                    point.mix(1).unit() - 0.5,
                    1.0,
                    point.mix(2).unit() - 0.5,
                );
                let mut frame = Frame::at(surface.frame.point(local));
                if align {
                    frame.rotation = surface.frame.rotation;
                }
                Place::at(frame)
            })
            .collect();
        // Keyed by the surface too, so two surfaces' copies differ.
        made.extend(copies(run, objects, &places).into_iter().map(
            |mut copy| {
                copy.id = run.id(surface.id, copy.id.0 as u32);
                copy.tags.clone_from(&surface.tags);
                copy
            },
        ));
    }
    run.put("objects", Value::Objects(made));
    Ok(())
}

const ON_SOCKETS: NodeKind = NodeKind {
    name: "on_sockets",
    doc: "The objects at a socket of each object that has it, \
          turned as the socket is",
    inputs: &[
        OBJECTS_IN,
        PortSpec::new(
            "on",
            ValueType::Objects,
            "The objects whose sockets to put them at",
        ),
        PortSpec::new(
            "socket",
            ValueType::Choice,
            "The socket's name, as its asset's file has it",
        )
        .listed(Listed::Sockets)
        .initial(Initial::Choice("lamp")),
    ],
    outputs: &[OBJECTS_OUT],
    run: on_sockets,
};

fn on_sockets(run: &mut Run) -> Result<(), Problem> {
    let name = run.choice("socket", None)?;
    let mut places = Vec::new();
    for object in run.objects("on")? {
        let Some(socket) = run
            .asset(&object.asset)
            .and_then(|asset| asset.sockets.get(&name).copied())
        else {
            continue;
        };
        // An asset stands on its frame, its box above it.
        let size = object.frame.scale;
        let rotation = object.frame.rotation;
        places.push(Place::at(Frame {
            translation: object.frame.translation
                + rotation * ((socket.at + Vec3::Y * 0.5) * size),
            rotation: rotation * socket.turn,
            scale: Vec3::ONE,
        }));
    }
    let made = copies(run, run.objects("objects")?, &places);
    run.put("objects", Value::Objects(made));
    Ok(())
}
