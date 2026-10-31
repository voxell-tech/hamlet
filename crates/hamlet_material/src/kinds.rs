//! The kinds of node a material graph is made of, each with the WGSL
//! it compiles to. Math works on the widest of its inputs, so one
//! node adds numbers, vectors or colours.

use ValueType::{
    Bool, Choice, Color, Integer, Number, Vector2, Vector3,
};
use bevy_math::{Vec3, Vec4};
use hamlet_blueprint::{
    Initial, NodeKind, PortSpec, Problem, Registry, Run, ValueType,
};

use crate::compile::{Emit, convert, width};

/// A kind of node and how it is compiled.
pub struct MaterialKind {
    pub kind: NodeKind,
    pub(crate) emit: fn(&mut Emit) -> Result<(), Problem>,
}

/// The kinds, by the group each is listed under.
const GROUPS: [(&str, &[MaterialKind]); 6] = [
    ("Inputs", INPUTS),
    ("Math", MATH),
    ("Vectors", VECTORS),
    ("Patterns", PATTERNS),
    ("Colours", COLOURS),
    ("Surface", &[SURFACE]),
];

/// The kinds a material graph may use.
pub fn registry() -> Registry {
    let mut registry = Registry::default();
    for (group, kinds) in GROUPS {
        for kind in kinds {
            registry.add(group, kind.kind);
        }
    }
    registry
}

pub(crate) fn get(name: &str) -> Option<&'static MaterialKind> {
    GROUPS
        .iter()
        .flat_map(|(_, kinds)| kinds.iter())
        .find(|kind| kind.kind.name == name)
}

/// What a node of a material does on the CPU: nothing, as its shader
/// works it out.
fn on_gpu(run: &mut Run) -> Result<(), Problem> {
    Err(run.invalid("", "is worked out by a material's shader"))
}

const fn kind(
    name: &'static str,
    doc: &'static str,
    inputs: &'static [PortSpec],
    outputs: &'static [PortSpec],
    emit: fn(&mut Emit) -> Result<(), Problem>,
) -> MaterialKind {
    MaterialKind {
        kind: NodeKind {
            name,
            doc,
            inputs,
            outputs,
            run: on_gpu,
        },
        emit,
    }
}

const fn number(
    name: &'static str,
    doc: &'static str,
    initial: f32,
) -> PortSpec {
    PortSpec::new(name, Number, doc).initial(Initial::Number(initial))
}

const fn color(
    name: &'static str,
    doc: &'static str,
    initial: Vec4,
) -> PortSpec {
    PortSpec::new(name, Color, doc).initial(Initial::Color(initial))
}

const fn out(
    name: &'static str,
    value_type: ValueType,
    doc: &'static str,
) -> PortSpec {
    PortSpec::new(name, value_type, doc)
}

/// Where a pattern is read, the point on the surface when unwired.
const AT: PortSpec = PortSpec::new(
    "vector",
    Vector3,
    "Where it is read; where on the surface, in the world, when \
     unwired",
);

const WHITE: Vec4 = Vec4::new(1.0, 1.0, 1.0, 1.0);
const BLACK: Vec4 = Vec4::new(0.0, 0.0, 0.0, 1.0);
const GREY: Vec4 = Vec4::new(0.8, 0.8, 0.8, 1.0);

/// The surface `surface` gives when the graph has no surface node:
/// the mesh's own colour, lit.
pub(crate) const PLAIN: &str =
    "Surface(i.color, 0.5, 0.0, vec3<f32>(0.0), i.normal, 0.0)";

/// A value of `value_type` with every axis `number`.
fn splat(number: &str, value_type: ValueType) -> String {
    convert(number, Number, value_type)
}

/// The wider of two types.
fn wider(a: ValueType, b: ValueType) -> ValueType {
    if width(b) > width(a) { b } else { a }
}

const INPUTS: &[MaterialKind] = &[
    kind(
        "uv",
        "Where on its texture the surface is, from 0 to 1 across it",
        &[],
        &[out("uv", Vector2, "Where on its texture")],
        |emit| {
            emit.put("uv", Vector2, "i.uv");
            Ok(())
        },
    ),
    kind(
        "position",
        "Where the surface is, in the world",
        &[],
        &[out("position", Vector3, "Where it is")],
        |emit| {
            emit.put("position", Vector3, "i.position");
            Ok(())
        },
    ),
    kind(
        "normal",
        "The way the surface faces, in the world",
        &[],
        &[out("normal", Vector3, "The way it faces")],
        |emit| {
            emit.put("normal", Vector3, "i.normal");
            Ok(())
        },
    ),
    kind(
        "view direction",
        "The way from the surface to the camera",
        &[],
        &[out("view", Vector3, "Toward the camera")],
        |emit| {
            emit.put("view", Vector3, "i.view");
            Ok(())
        },
    ),
    kind(
        "surface colour",
        "The colour the mesh gives: its palette colour, which keeps \
         the hamlet's look",
        &[],
        &[out("colour", Color, "The mesh's colour")],
        |emit| {
            emit.put("colour", Color, "i.color");
            Ok(())
        },
    ),
    kind(
        "time",
        "Seconds since the start, times `speed`",
        &[number("speed", "How fast it runs", 1.0)],
        &[out("time", Number, "The time")],
        |emit| {
            let speed = emit.input("speed", Number)?;
            emit.put("time", Number, &format!("i.time * {speed}"));
            Ok(())
        },
    ),
    kind(
        "number",
        "A number",
        &[number("value", "The number", 0.0)],
        &[out("value", Number, "The number")],
        |emit| {
            let value = emit.input("value", Number)?;
            emit.put("value", Number, &value);
            Ok(())
        },
    ),
    kind(
        "vector",
        "Three numbers",
        &[PortSpec::new("value", Vector3, "The vector")
            .initial(Initial::Point(Vec3::ZERO))],
        &[out("value", Vector3, "The vector")],
        |emit| {
            let value = emit.input("value", Vector3)?;
            emit.put("value", Vector3, &value);
            Ok(())
        },
    ),
    kind(
        "colour",
        "A colour",
        &[color("colour", "The colour", GREY)],
        &[out("colour", Color, "The colour")],
        |emit| {
            let value = emit.input("colour", Color)?;
            emit.put("colour", Color, &value);
            Ok(())
        },
    ),
];

/// Math's operations that take b too.
const BINARY: &[&str] = &[
    "add", "subtract", "multiply", "divide", "power", "min", "max",
    "modulo", "less", "greater",
];

const MATH: &[MaterialKind] = &[
    kind(
        "math",
        "a and b worked out axis by axis, as wide as the wider of \
         them. Sin, cos and tan are in radians; less and greater \
         give 1 or 0",
        &[
            PortSpec::new("op", Choice, "What to work out")
                .initial(Initial::Choice("add"))
                .choices(&[
                    "add",
                    "subtract",
                    "multiply",
                    "divide",
                    "power",
                    "min",
                    "max",
                    "modulo",
                    "less",
                    "greater",
                    "abs",
                    "floor",
                    "ceil",
                    "round",
                    "fract",
                    "sqrt",
                    "sin",
                    "cos",
                    "tan",
                    "negate",
                    "one minus",
                ])
                .names(),
            number("a", "The first value", 0.0),
            number("b", "The second value", 0.0).used_with(BINARY),
        ],
        &[out("value", Number, "The result, as wide as a and b")],
        |emit| {
            let op = emit.choice("op")?;
            let (_, a_type) = emit.any("a")?;
            let value_type = if BINARY.contains(&op.as_str()) {
                wider(a_type, emit.any("b")?.1)
            } else {
                a_type
            };
            let a = emit.input("a", value_type)?;
            let b = if BINARY.contains(&op.as_str()) {
                emit.input("b", value_type)?
            } else {
                String::new()
            };
            let zero = splat("0.0", value_type);
            let one = splat("1.0", value_type);
            let code = match op.as_str() {
                "add" => format!("{a} + {b}"),
                "subtract" => format!("{a} - {b}"),
                "multiply" => format!("{a} * {b}"),
                "divide" => format!("{a} / {b}"),
                "power" => format!("pow(abs({a}), {b})"),
                "min" => format!("min({a}, {b})"),
                "max" => format!("max({a}, {b})"),
                "modulo" => {
                    let a = emit.local("a", &a);
                    let b = emit.local("b", &b);
                    format!("{a} - {b} * floor({a} / {b})")
                }
                "less" => format!("select({zero}, {one}, {a} < {b})"),
                "greater" => {
                    format!("select({zero}, {one}, {a} > {b})")
                }
                "sqrt" => format!("sqrt(max({a}, {zero}))"),
                "negate" => format!("-({a})"),
                "one minus" => format!("{one} - {a}"),
                op @ ("abs" | "floor" | "ceil" | "round"
                | "fract" | "sin" | "cos" | "tan") => {
                    format!("{op}({a})")
                }
                _ => return Err(emit.invalid("op", "is not known")),
            };
            emit.put("value", value_type, &code);
            Ok(())
        },
    ),
    kind(
        "mix",
        "a and b blended by factor: a at 0, b at 1",
        &[
            number("a", "Where it starts", 0.0),
            number("b", "Where it ends", 1.0),
            number("factor", "How far from a to b", 0.5),
        ],
        &[out("value", Number, "The blend, as wide as a and b")],
        |emit| {
            let value_type =
                wider(emit.any("a")?.1, emit.any("b")?.1);
            let a = emit.input("a", value_type)?;
            let b = emit.input("b", value_type)?;
            let factor = emit.input("factor", value_type)?;
            emit.put(
                "value",
                value_type,
                &format!("mix({a}, {b}, {factor})"),
            );
            Ok(())
        },
    ),
    kind(
        "clamp",
        "A value kept between low and high",
        &[
            number("value", "The value", 0.0),
            number("low", "The least it may be", 0.0),
            number("high", "The most it may be", 1.0),
        ],
        &[out("value", Number, "The value, kept")],
        |emit| {
            let value_type = emit.any("value")?.1;
            let value = emit.input("value", value_type)?;
            let low = emit.input("low", value_type)?;
            let high = emit.input("high", value_type)?;
            emit.put(
                "value",
                value_type,
                &format!("clamp({value}, {low}, {high})"),
            );
            Ok(())
        },
    ),
    kind(
        "remap",
        "A value moved from one range to another",
        &[
            number("value", "The value", 0.5),
            number(
                "from low",
                "Where the range it is in starts",
                0.0,
            ),
            number("from high", "Where the range it is in ends", 1.0),
            number("to low", "Where the new range starts", 0.0),
            number("to high", "Where the new range ends", 1.0),
        ],
        &[out("value", Number, "The value in the new range")],
        |emit| {
            let value_type = emit.any("value")?.1;
            let value = emit.input("value", value_type)?;
            let from_low = emit.input("from low", value_type)?;
            let from_high = emit.input("from high", value_type)?;
            let to_low = emit.input("to low", value_type)?;
            let to_high = emit.input("to high", value_type)?;
            emit.put(
                "value",
                value_type,
                &format!(
                    "{to_low} + ({value} - {from_low}) / \
                     ({from_high} - {from_low}) * ({to_high} - \
                     {to_low})"
                ),
            );
            Ok(())
        },
    ),
    kind(
        "smoothstep",
        "0 below low, 1 above high, and a smooth rise between",
        &[
            number("low", "Where the rise starts", 0.0),
            number("high", "Where the rise ends", 1.0),
            number("value", "The value", 0.5),
        ],
        &[out("value", Number, "The rise")],
        |emit| {
            let value_type = emit.any("value")?.1;
            let value = emit.input("value", value_type)?;
            let low = emit.input("low", value_type)?;
            let high = emit.input("high", value_type)?;
            emit.put(
                "value",
                value_type,
                &format!("smoothstep({low}, {high}, {value})"),
            );
            Ok(())
        },
    ),
];

const VECTORS: &[MaterialKind] = &[
    kind(
        "split",
        "A vector or colour's axes, each as a number",
        &[color("vector", "The vector or colour", BLACK)],
        &[
            out("x", Number, "x, or red"),
            out("y", Number, "y, or green"),
            out("z", Number, "z, or blue"),
            out("w", Number, "w, or how opaque"),
        ],
        |emit| {
            let vector = emit.input("vector", Color)?;
            let vector = emit.local("vector", &vector);
            for axis in ["x", "y", "z", "w"] {
                emit.put(axis, Number, &format!("{vector}.{axis}"));
            }
            Ok(())
        },
    ),
    kind(
        "combine",
        "A vector and a colour from numbers",
        &[
            number("x", "x, or red", 0.0),
            number("y", "y, or green", 0.0),
            number("z", "z, or blue", 0.0),
            number("w", "w, or how opaque", 1.0),
        ],
        &[
            out("vector", Vector3, "x, y and z"),
            out("colour", Color, "x, y, z and w"),
        ],
        |emit| {
            let x = emit.input("x", Number)?;
            let y = emit.input("y", Number)?;
            let z = emit.input("z", Number)?;
            let w = emit.input("w", Number)?;
            emit.put(
                "vector",
                Vector3,
                &format!("vec3<f32>({x}, {y}, {z})"),
            );
            emit.put(
                "colour",
                Color,
                &format!("vec4<f32>({x}, {y}, {z}, {w})"),
            );
            Ok(())
        },
    ),
    kind(
        "vector math",
        "Two vectors worked out together. Dot, length and distance \
         give a number; the rest a vector",
        &[
            PortSpec::new("op", Choice, "What to work out")
                .initial(Initial::Choice("dot"))
                .choices(&[
                    "dot",
                    "cross",
                    "length",
                    "distance",
                    "normalize",
                    "reflect",
                ])
                .names(),
            PortSpec::new("a", Vector3, "The first vector")
                .initial(Initial::Point(Vec3::ZERO)),
            PortSpec::new("b", Vector3, "The second vector")
                .initial(Initial::Point(Vec3::Y))
                .used_with(&["dot", "cross", "distance", "reflect"]),
        ],
        &[
            out("vector", Vector3, "The vector worked out"),
            out("value", Number, "The number worked out"),
        ],
        |emit| {
            let op = emit.choice("op")?;
            let a = emit.input("a", Vector3)?;
            let b = emit.input("b", Vector3)?;
            let (vector, value) = match op.as_str() {
                "dot" => (None, format!("dot({a}, {b})")),
                "length" => (None, format!("length({a})")),
                "distance" => (None, format!("distance({a}, {b})")),
                "cross" => {
                    (Some(format!("cross({a}, {b})")), String::new())
                }
                "normalize" => {
                    (Some(format!("normalize({a})")), String::new())
                }
                "reflect" => (
                    Some(format!("reflect({a}, {b})")),
                    String::new(),
                ),
                _ => return Err(emit.invalid("op", "is not known")),
            };
            match vector {
                Some(vector) => {
                    let vector = emit.local("vector", &vector);
                    emit.put("vector", Vector3, &vector);
                    emit.put(
                        "value",
                        Number,
                        &format!("length({vector})"),
                    );
                }
                None => {
                    let value = emit.local("value", &value);
                    emit.put(
                        "vector",
                        Vector3,
                        &splat(&value, Vector3),
                    );
                    emit.put("value", Number, &value);
                }
            }
            Ok(())
        },
    ),
    kind(
        "tiling",
        "Where on its texture the surface is, repeated `tiling` \
         times and moved by `offset`",
        &[
            PortSpec::new(
                "uv",
                Vector2,
                "Where on its texture; the surface's own when \
                 unwired",
            ),
            PortSpec::new("tiling", Vector2, "How many times across")
                .initial(Initial::Point(Vec3::new(1.0, 1.0, 0.0))),
            PortSpec::new("offset", Vector2, "How far it is moved")
                .initial(Initial::Point(Vec3::ZERO)),
        ],
        &[out("uv", Vector2, "Where on the texture")],
        |emit| {
            let uv = emit.input_or("uv", "i.uv")?;
            let tiling = emit.input("tiling", Vector2)?;
            let offset = emit.input("offset", Vector2)?;
            emit.put(
                "uv",
                Vector2,
                &format!("{uv} * {tiling} + {offset}"),
            );
            Ok(())
        },
    ),
];

const PATTERNS: &[MaterialKind] = &[
    kind(
        "noise",
        "Smooth noise in layers, each finer and fainter than the \
         one before, from 0 to 1",
        &[
            AT,
            number("scale", "How many bumps to a unit", 4.0),
            PortSpec::new(
                "detail",
                Integer,
                "How many layers, up to 8",
            )
            .initial(Initial::Integer(4)),
            number(
                "roughness",
                "How strong each layer is against the last",
                0.5,
            ),
            number(
                "distortion",
                "How far the noise bends itself",
                0.0,
            ),
        ],
        &[
            out("value", Number, "The noise"),
            out("colour", Color, "Three noises, as a colour"),
        ],
        |emit| {
            let at = emit.input_or("vector", "i.position")?;
            let scale = emit.input("scale", Number)?;
            let detail = emit.input("detail", Number)?;
            let roughness = emit.input("roughness", Number)?;
            let distortion = emit.input("distortion", Number)?;
            let p = emit.local("p", &format!("{at} * {scale}"));
            let p = emit.local(
                "bent",
                &format!(
                    "{p} + {distortion} * vec3<f32>(hm_noise({p} + \
                     vec3<f32>(3.1)), hm_noise({p} + \
                     vec3<f32>(7.7)), hm_noise({p} + \
                     vec3<f32>(1.3)))"
                ),
            );
            let fbm = |offset: &str| {
                format!(
                    "hm_fbm({p} + vec3<f32>({offset}), {detail}, \
                     {roughness})"
                )
            };
            emit.put("value", Number, &fbm("0.0"));
            emit.put(
                "colour",
                Color,
                &format!(
                    "vec4<f32>({}, {}, {}, 1.0)",
                    fbm("0.0"),
                    fbm("5.2, 1.3, 2.8"),
                    fbm("1.7, 9.2, 3.1")
                ),
            );
            Ok(())
        },
    ),
    kind(
        "voronoi",
        "Points scattered one to a cell: the distance to the \
         nearest, and a colour picked for its cell",
        &[
            AT,
            number("scale", "How many cells to a unit", 5.0),
            number(
                "randomness",
                "How far points stray from a grid",
                1.0,
            ),
        ],
        &[
            out(
                "distance",
                Number,
                "The distance to the nearest point",
            ),
            out("colour", Color, "A colour picked for each cell"),
        ],
        |emit| {
            let at = emit.input_or("vector", "i.position")?;
            let scale = emit.input("scale", Number)?;
            let randomness = emit.input("randomness", Number)?;
            let cells = emit.local(
                "cells",
                &format!("hm_voronoi({at} * {scale}, {randomness})"),
            );
            emit.put("distance", Number, &format!("{cells}.x"));
            emit.put(
                "colour",
                Color,
                &format!("vec4<f32>({cells}.yzw, 1.0)"),
            );
            Ok(())
        },
    ),
    kind(
        "checker",
        "Cubes of two colours, every other one",
        &[
            AT,
            number("scale", "How many cubes to a unit", 4.0),
            color("colour a", "The first colour", WHITE),
            color("colour b", "The second colour", BLACK),
        ],
        &[
            out("colour", Color, "The colour"),
            out("value", Number, "1 on the first colour's cubes"),
        ],
        |emit| {
            let at = emit.input_or("vector", "i.position")?;
            let scale = emit.input("scale", Number)?;
            let a = emit.input("colour a", Color)?;
            let b = emit.input("colour b", Color)?;
            let value = emit.local(
                "value",
                &format!("1.0 - hm_checker({at} * {scale})"),
            );
            emit.put(
                "colour",
                Color,
                &format!("mix({b}, {a}, {value})"),
            );
            emit.put("value", Number, &value);
            Ok(())
        },
    ),
    kind(
        "gradient",
        "A rise from 0 to 1: along x, along x squared, around the z \
         axis, or out from the middle",
        &[
            AT,
            PortSpec::new("shape", Choice, "How it rises")
                .initial(Initial::Choice("linear"))
                .choices(&[
                    "linear",
                    "quadratic",
                    "radial",
                    "spherical",
                ]),
        ],
        &[out("value", Number, "The gradient")],
        |emit| {
            let at = emit.input_or("vector", "i.position")?;
            let p = emit.local("p", &at);
            let code = match emit.choice("shape")?.as_str() {
                "linear" => format!("clamp({p}.x, 0.0, 1.0)"),
                "quadratic" => {
                    format!("pow(clamp({p}.x, 0.0, 1.0), 2.0)")
                }
                "radial" => {
                    format!("atan2({p}.y, {p}.x) / 6.2831853 + 0.5")
                }
                "spherical" => format!("max(1.0 - length({p}), 0.0)"),
                _ => {
                    return Err(emit.invalid("shape", "is not known"));
                }
            };
            emit.put("value", Number, &code);
            Ok(())
        },
    ),
    kind(
        "wave",
        "Bands or rings, rising and falling",
        &[
            AT,
            number("scale", "How many waves to a unit", 5.0),
            number(
                "distortion",
                "How far noise bends the waves",
                0.0,
            ),
            PortSpec::new("shape", Choice, "Bands or rings")
                .initial(Initial::Choice("bands"))
                .choices(&["bands", "rings"]),
            PortSpec::new(
                "profile",
                Choice,
                "How each wave rises and falls",
            )
            .initial(Initial::Choice("sine"))
            .choices(&["sine", "saw"]),
        ],
        &[out("value", Number, "The waves, from 0 to 1")],
        |emit| {
            let at = emit.input_or("vector", "i.position")?;
            let scale = emit.input("scale", Number)?;
            let distortion = emit.input("distortion", Number)?;
            let p = emit.local("p", &format!("{at} * {scale}"));
            let along = match emit.choice("shape")?.as_str() {
                "rings" => format!("length({p})"),
                _ => format!("({p}.x + {p}.y + {p}.z)"),
            };
            let d = emit.local(
                "d",
                &format!("{along} + {distortion} * hm_noise({p})"),
            );
            let code = match emit.choice("profile")?.as_str() {
                "saw" => format!("fract({d})"),
                _ => format!("0.5 + 0.5 * sin({d} * 6.2831853)"),
            };
            emit.put("value", Number, &code);
            Ok(())
        },
    ),
    kind(
        "brick",
        "Bricks laid in rows, every other row moved along, in mortar",
        &[
            PortSpec::new(
                "uv",
                Vector2,
                "Where it is read; where on the texture when unwired",
            ),
            number("scale", "How many rows to a unit", 4.0),
            number(
                "width",
                "How long a brick is, against its height",
                2.0,
            ),
            number(
                "mortar",
                "How wide the mortar is, against a brick's height",
                0.1,
            ),
            number(
                "offset",
                "How far every other row is moved, from 0 to 1",
                0.5,
            ),
            color(
                "colour a",
                "One colour of brick",
                Vec4::new(0.55, 0.25, 0.18, 1.0),
            ),
            color(
                "colour b",
                "The other colour of brick",
                Vec4::new(0.45, 0.2, 0.15, 1.0),
            ),
            color(
                "mortar colour",
                "The mortar's colour",
                Vec4::new(0.8, 0.78, 0.72, 1.0),
            ),
        ],
        &[
            out("colour", Color, "The colour"),
            out("value", Number, "1 on a brick, 0 in the mortar"),
            out("cell", Number, "A number picked for each brick"),
        ],
        |emit| {
            let uv = emit.input_or("uv", "i.uv")?;
            let scale = emit.input("scale", Number)?;
            let width = emit.input("width", Number)?;
            let mortar = emit.input("mortar", Number)?;
            let offset = emit.input("offset", Number)?;
            let a = emit.input("colour a", Color)?;
            let b = emit.input("colour b", Color)?;
            let mortar_colour = emit.input("mortar colour", Color)?;
            let bricks = emit.local(
                "bricks",
                &format!(
                    "hm_brick({uv} * {scale}, {width}, 1.0, \
                     {mortar}, {offset})"
                ),
            );
            emit.put(
                "colour",
                Color,
                &format!(
                    "mix({mortar_colour}, mix({a}, {b}, \
                     {bricks}.y), {bricks}.x)"
                ),
            );
            emit.put("value", Number, &format!("{bricks}.x"));
            emit.put("cell", Number, &format!("{bricks}.y"));
            Ok(())
        },
    ),
];

const COLOURS: &[MaterialKind] = &[
    kind(
        "blend",
        "A layer over a base, by factor, as a paint program blends",
        &[
            PortSpec::new("mode", Choice, "How the layer is blended")
                .initial(Initial::Choice("mix"))
                .choices(&[
                    "mix", "multiply", "screen", "overlay", "add",
                    "subtract", "darken", "lighten",
                ])
                .names(),
            color("base", "The colour under", GREY),
            color("layer", "The colour over", WHITE),
            number(
                "factor",
                "How much of the layer, from 0 to 1",
                1.0,
            ),
        ],
        &[out("colour", Color, "The blend")],
        |emit| {
            let base = emit.input("base", Color)?;
            let layer = emit.input("layer", Color)?;
            let factor = emit.input("factor", Number)?;
            let base = emit.local("base", &base);
            let layer = emit.local("layer", &layer);
            let (b, l) =
                (format!("{base}.rgb"), format!("{layer}.rgb"));
            let blended = match emit.choice("mode")?.as_str() {
                "mix" => l.clone(),
                "multiply" => format!("{b} * {l}"),
                "screen" => {
                    format!("1.0 - (1.0 - {b}) * (1.0 - {l})")
                }
                "overlay" => format!("hm_overlay({b}, {l})"),
                "add" => format!("{b} + {l}"),
                "subtract" => {
                    format!("max({b} - {l}, vec3<f32>(0.0))")
                }
                "darken" => format!("min({b}, {l})"),
                "lighten" => format!("max({b}, {l})"),
                _ => return Err(emit.invalid("mode", "is not known")),
            };
            emit.put(
                "colour",
                Color,
                &format!(
                    "vec4<f32>(mix({b}, {blended}, clamp({factor}, \
                     0.0, 1.0)), {base}.a)"
                ),
            );
            Ok(())
        },
    ),
    kind(
        "hsv",
        "A colour with its hue turned and its saturation and value \
         scaled",
        &[
            color("colour", "The colour", GREY),
            number(
                "hue",
                "How far round the hue turns, in turns",
                0.0,
            ),
            number("saturation", "How much richer", 1.0),
            number("value", "How much brighter", 1.0),
        ],
        &[out("colour", Color, "The colour changed")],
        |emit| {
            let colour = emit.input("colour", Color)?;
            let hue = emit.input("hue", Number)?;
            let saturation = emit.input("saturation", Number)?;
            let value = emit.input("value", Number)?;
            let colour = emit.local("colour", &colour);
            let hsv = emit.local(
                "hsv",
                &format!("hm_rgb_to_hsv({colour}.rgb)"),
            );
            emit.put(
                "colour",
                Color,
                &format!(
                    "vec4<f32>(hm_hsv_to_rgb(vec3<f32>(fract({hsv}.\
                     x + {hue}), clamp({hsv}.y * {saturation}, 0.0, \
                     1.0), {hsv}.z * {value})), {colour}.a)"
                ),
            );
            Ok(())
        },
    ),
    kind(
        "ramp",
        "A number turned into a colour: colour a at `from`, colour \
         b at `to`, and a blend between",
        &[
            number("factor", "The number", 0.5),
            number("from", "Where colour a is", 0.0),
            color("colour a", "The colour at from", BLACK),
            number("to", "Where colour b is", 1.0),
            color("colour b", "The colour at to", WHITE),
            PortSpec::new(
                "blend",
                Choice,
                "How it goes from one to the other",
            )
            .initial(Initial::Choice("linear"))
            .choices(&["linear", "smooth", "constant"]),
        ],
        &[out("colour", Color, "The colour")],
        |emit| {
            let factor = emit.input("factor", Number)?;
            let from = emit.input("from", Number)?;
            let to = emit.input("to", Number)?;
            let a = emit.input("colour a", Color)?;
            let b = emit.input("colour b", Color)?;
            let t = match emit.choice("blend")?.as_str() {
                "smooth" => {
                    format!("smoothstep({from}, {to}, {factor})")
                }
                "constant" => format!("step({to}, {factor})"),
                _ => format!(
                    "clamp(({factor} - {from}) / ({to} - {from}), \
                     0.0, 1.0)"
                ),
            };
            emit.put("colour", Color, &format!("mix({a}, {b}, {t})"));
            Ok(())
        },
    ),
    kind(
        "fresnel",
        "How edge-on the surface is to the camera: 0 facing it, \
         rising to 1 at its edges",
        &[
            number("power", "How close to the edges it rises", 5.0),
            PortSpec::new(
                "normal",
                Vector3,
                "The way the surface faces; its own when unwired",
            ),
        ],
        &[out("value", Number, "How edge-on")],
        |emit| {
            let power = emit.input("power", Number)?;
            let normal = emit.input_or("normal", "i.normal")?;
            emit.put(
                "value",
                Number,
                &format!(
                    "pow(1.0 - clamp(dot(normalize({normal}), \
                     i.view), 0.0, 1.0), {power})"
                ),
            );
            Ok(())
        },
    ),
];

/// The surface the material gives, lit by the realm's light.
pub(crate) const SURFACE: MaterialKind = kind(
    "surface",
    "What the material gives: its colour, how rough and metallic it \
     is, its glow, and the way it faces. Unlit shows the colour and \
     glow as they are, for effects",
    &[
        color("base colour", "Its colour", GREY),
        number("roughness", "From 0, a mirror, to 1, chalk", 0.5),
        number(
            "metallic",
            "From 0, paint or stone, to 1, metal",
            0.0,
        ),
        color("emission", "The light it gives off", BLACK),
        PortSpec::new(
            "normal",
            Vector3,
            "The way it faces, in the world; the mesh's when unwired",
        ),
        PortSpec::new("unlit", Bool, "Shown without light")
            .initial(Initial::Bool(false)),
    ],
    &[],
    |emit| {
        let base = emit.input("base colour", Color)?;
        let roughness = emit.input("roughness", Number)?;
        let metallic = emit.input("metallic", Number)?;
        let emission = emit.input("emission", Vector3)?;
        let normal = emit.input_or("normal", "i.normal")?;
        let unlit = emit.input("unlit", Number)?;
        emit.put_surface(&format!(
            "Surface({base}, clamp({roughness}, 0.0, 1.0), \
             clamp({metallic}, 0.0, 1.0), {emission}, \
             normalize({normal}), {unlit})"
        ));
        Ok(())
    },
);
