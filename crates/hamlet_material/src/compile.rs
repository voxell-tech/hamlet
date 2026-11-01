//! A material graph compiled to WGSL: a function from what a surface
//! is read from to what it gives, and one that shows what any node
//! gives, for its swatch. Every value set on the graph is read from a
//! slot, so a value changed is a slot written, not a shader compiled
//! again.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use bevy_math::Vec4;
use hamlet_graph::{
    Constant, Does, Error, Graph, Node, NodeId, PortSpec, Setting,
    ValueType,
};

use crate::kinds::{self, MaterialKind, SURFACE};

/// The slots a graph may set; slot 0 is the time.
pub const SLOTS: usize = 256;

/// The WGSL every compiled material shares.
pub const LIBRARY: &str = include_str!("library.wgsl");

/// Where a slot's value comes from.
#[derive(Clone, PartialEq, Debug)]
enum Source {
    /// An input of a node that nothing is wired to.
    Input { node: NodeId, input: &'static str },
    /// An input of the graph, at its default.
    Parent(String),
}

/// A graph as a shader, and how to fill its slots.
#[derive(Clone, PartialEq, Debug)]
pub struct Compiled {
    /// [`LIBRARY`], then `surface(i: SurfaceIn) -> Surface` and
    /// `preview(which: u32, i: SurfaceIn) -> vec4<f32>`.
    pub code: String,
    /// Each slot from 1 on, by where its value comes from.
    sources: Vec<Source>,
    /// The nodes that could not be compiled, each with why. What
    /// needs them takes its default.
    pub errors: Vec<Error>,
    /// Whether what it gives changes with the time, so its slots are
    /// filled again every frame.
    reads_time: bool,
}

impl Compiled {
    /// The value of every slot as `graph` is set now, the time in
    /// slot 0 at `time`. `graph` is the one compiled, or one only its
    /// values changed in.
    pub fn values(&self, graph: &Graph, time: f32) -> Vec<Vec4> {
        let mut values = vec![Vec4::ZERO; SLOTS];
        values[0].x = time;
        for (at, source) in self.sources.iter().enumerate() {
            let constant = match source {
                Source::Input { node, input } => graph
                    .node(*node)
                    .and_then(|found| set_to(graph, found, input)),
                Source::Parent(name) => graph
                    .inputs
                    .iter()
                    .find(|parent| parent.name == *name)
                    .and_then(|parent| parent.default.clone()),
            };
            values[at + 1] = constant.map_or(Vec4::ZERO, as_slot);
        }
        values
    }

    pub fn reads_time(&self) -> bool {
        self.reads_time
    }

    /// The number `preview` shows `node` for, if it shows it.
    pub fn which(node: NodeId) -> u32 {
        node.0
    }
}

/// What an unwired input of `node` is set to, or its kind's initial
/// value.
fn set_to(
    graph: &Graph,
    node: &Node,
    input: &str,
) -> Option<Constant> {
    match node.settings.get(input) {
        Some(Setting::Fixed(constant)) => Some(constant.clone()),
        Some(Setting::FromParent(name)) => graph
            .inputs
            .iter()
            .find(|parent| parent.name == *name)
            .and_then(|parent| parent.default.clone()),
        _ => {
            let Does::Kind(kind) = &node.does else {
                return None;
            };
            kinds::get(kind)?
                .kind
                .inputs
                .iter()
                .find(|spec| spec.name == input)?
                .initial
                .map(Constant::from)
        }
    }
}

fn as_slot(constant: Constant) -> Vec4 {
    match constant {
        Constant::Number(number) => Vec4::new(number, 0.0, 0.0, 0.0),
        Constant::Integer(whole) => {
            Vec4::new(whole as f32, 0.0, 0.0, 0.0)
        }
        Constant::Bool(flag) => {
            Vec4::new(if flag { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0)
        }
        Constant::Point(point) => point.extend(0.0),
        Constant::Color(color) => color,
        Constant::Choice(_) => Vec4::ZERO,
    }
}

/// The WGSL type of a value of `value_type`.
pub(crate) fn wgsl(value_type: ValueType) -> &'static str {
    match value_type {
        ValueType::Vector2 => "vec2<f32>",
        ValueType::Vector3 => "vec3<f32>",
        ValueType::Color => "vec4<f32>",
        _ => "f32",
    }
}

/// `code`, of type `from`, as a value of type `to`: a number fills
/// every axis, a vector gains zeros and an opaque alpha, and a longer
/// one drops its last axes.
pub(crate) fn convert(
    code: &str,
    from: ValueType,
    to: ValueType,
) -> String {
    use ValueType::{Color, Vector2, Vector3};
    match (from, to) {
        _ if from == to => code.to_owned(),
        (Vector2 | Vector3 | Color, ValueType::Number) => {
            format!("({code}).x")
        }
        (_, Vector2) if from == Vector3 || from == Color => {
            format!("({code}).xy")
        }
        (Color, Vector3) => format!("({code}).xyz"),
        (Vector2, Vector3) => format!("vec3<f32>({code}, 0.0)"),
        (Vector2, Color) => format!("vec4<f32>({code}, 0.0, 1.0)"),
        (Vector3, Color) => format!("vec4<f32>({code}, 1.0)"),
        (_, Color) => format!("vec4<f32>(vec3<f32>({code}), 1.0)"),
        (_, to) => format!("{}({code})", wgsl(to)),
    }
}

/// How wide a type is, to work out math on the widest of its inputs.
pub(crate) fn width(value_type: ValueType) -> u8 {
    match value_type {
        ValueType::Vector2 => 2,
        ValueType::Vector3 => 3,
        ValueType::Color => 4,
        _ => 1,
    }
}

/// A port's variable in the shader.
fn variable(node: NodeId, port: &str) -> String {
    let port: String = port
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    format!("v{}_{port}", node.0)
}

/// The nodes of `graph` in an order where each comes after those
/// wired into it.
fn order(graph: &Graph) -> Result<Vec<NodeId>, Error> {
    let mut placed = BTreeSet::new();
    let mut order = Vec::new();
    let mut left: Vec<NodeId> =
        graph.nodes.iter().map(|node| node.id).collect();
    while !left.is_empty() {
        let before = left.len();
        left.retain(|node| {
            let ready = graph.wires.iter().all(|wire| {
                wire.to.node != *node
                    || placed.contains(&wire.from.node)
            });
            if ready {
                placed.insert(*node);
                order.push(*node);
            }
            !ready
        });
        if left.len() == before {
            return Err(Error::Loop(left));
        }
    }
    Ok(order)
}

/// Compiles `graph`. A node that fails is left out, and what needs it
/// takes its default; a loop fails the whole graph.
pub fn compile(graph: &Graph) -> Result<Compiled, Error> {
    let order = order(graph)?;
    let mut compiler = Compiler {
        graph,
        sources: Vec::new(),
        parents: BTreeMap::new(),
        given: BTreeMap::new(),
        body: String::new(),
        errors: Vec::new(),
    };
    let mut shown = Vec::new();
    let mut surface = None;
    for id in order {
        let Some(node) = graph.node(id) else {
            continue;
        };
        let Does::Kind(name) = &node.does else {
            compiler.errors.push(Error::Invalid {
                node: id,
                port: String::new(),
                reason: "runs a graph, which a material cannot yet",
            });
            continue;
        };
        let Some(kind) = kinds::get(name) else {
            compiler.errors.push(Error::UnknownKind {
                node: id,
                kind: name.clone(),
            });
            continue;
        };
        let mut emit = Emit {
            compiler: &mut compiler,
            node,
            kind,
            lines: String::new(),
            given: Vec::new(),
        };
        match (kind.emit)(&mut emit) {
            Ok(()) => {
                let Emit { lines, given, .. } = emit;
                compiler.body.push_str(&lines);
                if let Some((port, value_type)) = given.first() {
                    shown.push((id, variable(id, port), *value_type));
                }
                for (port, value_type) in given {
                    compiler.given.insert((id, port), value_type);
                }
                if name == SURFACE.kind.name && surface.is_none() {
                    surface = Some(id);
                }
            }
            Err(error) => compiler.errors.push(error),
        }
    }
    let Compiler {
        sources,
        body,
        errors,
        ..
    } = compiler;
    let reads_time = body.contains("i.time");
    let mut code = String::from(LIBRARY);
    code.push_str(
        "\n// The graph.\n\n@group(GROUP) @binding(SLOTS_BINDING) \
         var<uniform> slots: Slots;\n\n",
    );
    let _ = writeln!(code, "fn surface(i: SurfaceIn) -> Surface {{");
    code.push_str(&body);
    match surface {
        Some(node) => {
            let _ = writeln!(
                code,
                "    return {};",
                variable(node, "out")
            );
        }
        None => {
            let _ = writeln!(code, "    return {};", kinds::PLAIN);
        }
    }
    code.push_str("}\n\n");
    let _ = writeln!(
        code,
        "fn preview(which: u32, i: SurfaceIn) -> vec4<f32> {{"
    );
    code.push_str(&body);
    code.push_str("    switch which {\n");
    for (node, variable, value_type) in &shown {
        let _ = writeln!(
            code,
            "        case {}u: {{ return hm_show{}({variable}); }}",
            Compiled::which(*node),
            width(*value_type)
        );
    }
    if let Some(node) = surface {
        let _ = writeln!(
            code,
            "        case {}u: {{ return {}.base_color; }}",
            Compiled::which(node),
            variable(node, "out")
        );
    }
    code.push_str(
        "        default: { return vec4<f32>(0.0); }\n    }\n}\n",
    );
    Ok(Compiled {
        code,
        sources,
        errors,
        reads_time,
    })
}

struct Compiler<'a> {
    graph: &'a Graph,
    sources: Vec<Source>,
    /// The slot each graph input is read from.
    parents: BTreeMap<String, usize>,
    /// The type of each output compiled so far.
    given: BTreeMap<(NodeId, String), ValueType>,
    body: String,
    errors: Vec<Error>,
}

impl Compiler<'_> {
    fn slot(
        &mut self,
        source: Source,
    ) -> Result<usize, &'static str> {
        if let Source::Parent(name) = &source
            && let Some(at) = self.parents.get(name)
        {
            return Ok(*at);
        }
        if self.sources.len() + 1 >= SLOTS {
            return Err("needs more values than a material holds");
        }
        self.sources.push(source.clone());
        let at = self.sources.len();
        if let Source::Parent(name) = source {
            self.parents.insert(name, at);
        }
        Ok(at)
    }
}

/// What a kind's `emit` writes one node with.
pub(crate) struct Emit<'a, 'b> {
    compiler: &'b mut Compiler<'a>,
    node: &'b Node,
    kind: &'static MaterialKind,
    lines: String,
    given: Vec<(String, ValueType)>,
}

impl Emit<'_, '_> {
    fn spec(&self, port: &str) -> Result<&'static PortSpec, Error> {
        self.kind
            .kind
            .inputs
            .iter()
            .find(|spec| spec.name == port)
            .ok_or_else(|| self.invalid(port, "is not an input"))
    }

    pub(crate) fn invalid(
        &self,
        port: &str,
        reason: &'static str,
    ) -> Error {
        Error::Invalid {
            node: self.node.id,
            port: port.to_owned(),
            reason,
        }
    }

    /// The wire into `port`, as the output's variable and type, or
    /// none for an unwired one. A wire from a node that failed counts
    /// as none.
    fn wired(&self, port: &str) -> Option<(String, ValueType)> {
        let wire = self.compiler.graph.wires.iter().find(|wire| {
            wire.to.node == self.node.id && wire.to.name == port
        })?;
        let value_type = self
            .compiler
            .given
            .get(&(wire.from.node, wire.from.name.clone()))?;
        Some((variable(wire.from.node, &wire.from.name), *value_type))
    }

    /// `port` as it comes, its type the wire's, else its own.
    pub(crate) fn any(
        &mut self,
        port: &str,
    ) -> Result<(String, ValueType), Error> {
        if let Some(wired) = self.wired(port) {
            return Ok(wired);
        }
        let spec = self.spec(port)?;
        Ok((self.unwired(spec)?, spec.value_type))
    }

    /// `port` as a value of `value_type`.
    pub(crate) fn input(
        &mut self,
        port: &str,
        value_type: ValueType,
    ) -> Result<String, Error> {
        let (code, from) = self.any(port)?;
        Ok(convert(&code, from, value_type))
    }

    /// `port` as a value of its own type, or `otherwise` when it is
    /// not wired.
    pub(crate) fn input_or(
        &mut self,
        port: &str,
        otherwise: &str,
    ) -> Result<String, Error> {
        let spec = self.spec(port)?;
        Ok(match self.wired(port) {
            Some((code, from)) => {
                convert(&code, from, spec.value_type)
            }
            None => otherwise.to_owned(),
        })
    }

    /// The choice `port` is set to, fixed when the graph is compiled.
    pub(crate) fn choice(&self, port: &str) -> Result<String, Error> {
        let spec = self.spec(port)?;
        Ok(match self.node.settings.get(port) {
            Some(Setting::Fixed(Constant::Choice(choice))) => {
                choice.clone()
            }
            _ => match spec.initial {
                Some(hamlet_graph::Initial::Choice(choice)) => {
                    choice.to_owned()
                }
                _ => spec
                    .choices
                    .first()
                    .copied()
                    .unwrap_or_default()
                    .to_owned(),
            },
        })
    }

    /// The slot an unwired input is read from, as its type.
    fn unwired(
        &mut self,
        spec: &'static PortSpec,
    ) -> Result<String, Error> {
        let source = match self.node.settings.get(spec.name) {
            Some(Setting::FromParent(name)) => {
                Source::Parent(name.clone())
            }
            _ => Source::Input {
                node: self.node.id,
                input: spec.name,
            },
        };
        let at = self
            .compiler
            .slot(source)
            .map_err(|reason| self.invalid(spec.name, reason))?;
        Ok(match spec.value_type {
            ValueType::Vector2 => format!("slots.v[{at}].xy"),
            ValueType::Vector3 => format!("slots.v[{at}].xyz"),
            ValueType::Color => format!("slots.v[{at}]"),
            _ => format!("slots.v[{at}].x"),
        })
    }

    /// Gives `port` as `code`, of `value_type`.
    pub(crate) fn put(
        &mut self,
        port: &str,
        value_type: ValueType,
        code: &str,
    ) {
        let _ = writeln!(
            self.lines,
            "    let {}: {} = {code};",
            variable(self.node.id, port),
            wgsl(value_type)
        );
        if self.kind.kind.outputs.iter().any(|spec| spec.name == port)
        {
            self.given.push((port.to_owned(), value_type));
        }
    }

    /// Gives the surface as `code`, which `surface` returns.
    pub(crate) fn put_surface(&mut self, code: &str) {
        let _ = writeln!(
            self.lines,
            "    let {}: Surface = {code};",
            variable(self.node.id, "out")
        );
    }

    /// A variable of this node's own, for a value worked out on the
    /// way.
    pub(crate) fn local(&mut self, name: &str, code: &str) -> String {
        let variable = variable(self.node.id, &format!("_{name}"));
        let _ = writeln!(self.lines, "    let {variable} = {code};");
        variable
    }
}
