//! A plugin's shader: WGSL for one function that colours a point of a
//! shape, checked with naga against the interface of
//! `hamlet_realm_api::scene` before the realm makes it a material.

use std::collections::HashMap;
use std::error::Error;

use hamlet_realm_api::scene::{INTERFACE, SHADE, SceneShader};
use naga::valid::{Capabilities, ValidationFlags, Validator};
use naga::{
    AddressSpace, BinaryOperator, Block, Expression, Function,
    Handle, Literal, LocalVariable, Module, Scalar, ScalarKind,
    SourceLocation, Statement, TypeInner, VectorSize,
};

use crate::limits::Limits;

/// What the check puts after a plugin's source: the names the
/// material's shader declares too, so that a plugin cannot take one,
/// and an entry point that calls `shade` as the material's does.
const STAND_IN: &str = "
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
}

struct HamletGlobals {
    time: f32,
}

@group(0) @binding(0) var<uniform> globals: HamletGlobals;
@group(0) @binding(1) var<uniform> hamlet_params: HamletParams;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let at = in.position;
    return shade(Surface(at.xy, at.xyz, vec3<f32>(0.0, 1.0, 0.0), \
                        globals.time));
}
";

/// The globals of the stand-in, which a plugin's source may use.
const OWN: [&str; 2] = ["globals", "hamlet_params"];

/// Why a loop is refused.
const LOOPS: &str = "a loop must be a `for` over a counter with \
                     fixed bounds, such as `for (var i = 0; i < 8; \
                     i++)`";

/// The shader `source` of a plugin, kept as `id`, or why it is
/// refused.
pub fn checked(
    id: &str,
    source: &str,
    limits: &Limits,
) -> Result<SceneShader, String> {
    if source.len() > limits.shader_bytes {
        return Err(format!(
            "the shader has {} bytes, the most is {}",
            source.len(),
            limits.shader_bytes
        ));
    }
    if source.contains('#') {
        return Err("a shader has no `#`: it is plain WGSL".into());
    }
    if source.contains("naga_oil") {
        return Err("`naga_oil` is a name of the host's".into());
    }
    let whole = format!("{source}\n{INTERFACE}\n{STAND_IN}");
    let lines = source.lines().count() as u32;
    let module = naga::front::wgsl::parse_str(&whole)
        .map_err(|e| at(e.location(&whole), lines, e.message()))?;
    own(&module)?;
    Validator::new(ValidationFlags::all(), Capabilities::empty())
        .validate(&module)
        .map_err(|e| {
            at(e.location(&whole), lines, &chain(e.as_inner()))
        })?;
    runs(&module, limits.shader_loop_runs)?;
    Ok(SceneShader {
        id: id.into(),
        source: source.into(),
    })
}

/// `message`, with the line of the plugin's source it is about.
fn at(
    location: Option<SourceLocation>,
    lines: u32,
    message: &str,
) -> String {
    match location {
        Some(at) if at.line_number <= lines => {
            format!("line {}: {message}", at.line_number)
        }
        _ => message.into(),
    }
}

/// An error and the errors under it, on one line.
fn chain(error: &dyn Error) -> String {
    let mut words = error.to_string();
    let mut under = error.source();
    while let Some(next) = under {
        words.push_str(": ");
        words.push_str(&next.to_string());
        under = next.source();
    }
    words
}

/// Whether the module holds only the plugin's functions and the
/// stand-in's: no entry point, binding or override of its own, and
/// `shade` as the interface has it.
fn own(module: &Module) -> Result<(), String> {
    if let Some(entry) =
        module.entry_points.iter().find(|e| e.name != "fragment")
    {
        return Err(format!(
            "`{}` is an entry point; a shader has none of its own",
            entry.name
        ));
    }
    for (_, global) in module.global_variables.iter() {
        let name = global.name.as_deref().unwrap_or("a global");
        let stand_in = OWN.contains(&name);
        if !stand_in && global.space != AddressSpace::Private {
            return Err(format!(
                "`{name}` is a binding; a shader has none of its own"
            ));
        }
    }
    if !module.overrides.is_empty() {
        return Err("a shader has no `override` of its own".into());
    }
    let shade = module
        .functions
        .iter()
        .map(|(_, function)| function)
        .find(|function| function.name.as_deref() == Some(SHADE));
    let surface =
        |ty| module.types[ty].name.as_deref() == Some("Surface");
    let colour = |ty| {
        module.types[ty].inner
            == TypeInner::Vector {
                size: VectorSize::Quad,
                scalar: Scalar::F32,
            }
    };
    match shade {
        Some(function)
            if function.arguments.len() == 1
                && surface(function.arguments[0].ty)
                && function
                    .result
                    .as_ref()
                    .is_some_and(|r| colour(r.ty)) =>
        {
            Ok(())
        }
        _ => Err(format!(
            "no `fn {SHADE}(surface: Surface) -> vec4<f32>`"
        )),
    }
}

/// Whether every loop of the plugin's functions has a count, and the
/// loops a point runs through, nested and called, run at most `most`
/// times. The functions come after those they call.
fn runs(module: &Module, most: u64) -> Result<(), String> {
    let mut costs = HashMap::new();
    for (handle, function) in module.functions.iter() {
        let cost = Walk {
            module,
            function,
            costs: &costs,
        }
        .block(&function.body)?;
        if cost > most {
            let name =
                function.name.as_deref().unwrap_or("a function");
            return Err(format!(
                "the loops of `{name}` may run {cost} times for a \
                 point, the most is {most}"
            ));
        }
        costs.insert(handle, cost);
    }
    Ok(())
}

/// One function of the module, walked for the times its loops run.
struct Walk<'a> {
    module: &'a Module,
    function: &'a Function,
    /// The times the loops of each function before it run.
    costs: &'a HashMap<Handle<Function>, u64>,
}

impl Walk<'_> {
    /// The times the loops of `block` run at most, one for none.
    fn block(&self, block: &Block) -> Result<u64, String> {
        let mut most = 1u64;
        for (at, statement) in block.iter().enumerate() {
            let runs = match statement {
                Statement::Block(inner) => self.block(inner)?,
                Statement::If { accept, reject, .. } => {
                    self.block(accept)?.max(self.block(reject)?)
                }
                Statement::Switch { cases, .. } => {
                    cases.iter().try_fold(0u64, |sum, case| {
                        Ok::<_, String>(
                            sum.saturating_add(
                                self.block(&case.body)?,
                            ),
                        )
                    })?
                }
                Statement::Loop {
                    body,
                    continuing,
                    break_if,
                } => {
                    let count = self.count(
                        &block[..at],
                        body,
                        continuing,
                        *break_if,
                    )?;
                    let inner = self
                        .block(body)?
                        .max(self.block(continuing)?);
                    count.max(1).saturating_mul(inner)
                }
                Statement::Call { function, .. } => {
                    self.costs.get(function).copied().ok_or(
                        "a function is called before it is checked",
                    )?
                }
                _ => 1,
            };
            most = most.max(runs);
        }
        Ok(most)
    }

    /// The times a loop runs, if it is a `for` over a counter that
    /// only its update changes, with a start and a bound that are
    /// numbers. `before` is what its block does before it.
    fn count(
        &self,
        before: &[Statement],
        body: &Block,
        continuing: &Block,
        break_if: Option<Handle<Expression>>,
    ) -> Result<u64, String> {
        if break_if.is_some() {
            return Err(LOOPS.into());
        }
        let first = body.iter().find(|statement| {
            !matches!(statement, Statement::Emit(_))
        });
        let Some(Statement::If {
            condition,
            accept,
            reject,
        }) = first
        else {
            return Err(LOOPS.into());
        };
        if !accept.is_empty()
            || !matches!(&reject[..], [Statement::Break])
        {
            return Err(LOOPS.into());
        }
        let Expression::Binary { op, left, right } =
            self.function.expressions[*condition]
        else {
            return Err(LOOPS.into());
        };
        let (counter, bound, op) =
            match (self.loaded(left), self.number(right)) {
                (Some(counter), Some(bound)) => (counter, bound, op),
                _ => match (self.number(left), self.loaded(right)) {
                    (Some(bound), Some(counter)) => {
                        (counter, bound, flipped(op))
                    }
                    _ => return Err(LOOPS.into()),
                },
            };
        let local = &self.function.local_variables[counter];
        // The counter starts from a number given where it is
        // declared: as the variable's own value, or stored
        // just before the loop where the loop is in another.
        let set = before.iter().rev().find(|statement| {
            self.writes(std::slice::from_ref(*statement), counter) > 0
        });
        let start = match set {
            Some(Statement::Store { pointer, value })
                if self.local(*pointer) == Some(counter) =>
            {
                self.number(*value)
            }
            Some(_) => None,
            None => local.init.and_then(|init| self.number(init)),
        }
        .ok_or(LOOPS)?;
        let step = self.step(continuing, counter)?;
        // Nothing else changes it, in the loop or around it.
        let writes = self.writes(&self.function.body, counter);
        if writes != 1 + usize::from(set.is_some()) {
            return Err("the counter of a loop is changed outside \
                        its update"
                .into());
        }
        let (count, last) = match op {
            BinaryOperator::Less if step > 0 => {
                over(bound - start, step, start)
            }
            BinaryOperator::LessEqual if step > 0 => {
                over(bound - start + 1, step, start)
            }
            BinaryOperator::Greater if step < 0 => {
                over(start - bound, step, start)
            }
            BinaryOperator::GreaterEqual if step < 0 => {
                over(start - bound + 1, step, start)
            }
            _ => return Err(LOOPS.into()),
        };
        // The value that ends the loop must be one the counter can
        // hold, or it wraps round and the loop goes on.
        let (low, high) = self.span(local).ok_or(LOOPS)?;
        if last < low || last > high {
            return Err(LOOPS.into());
        }
        Ok(count)
    }

    /// The step the update of a loop adds to `counter`: its only
    /// statement but the emits, a store of the counter plus or minus
    /// a number.
    fn step(
        &self,
        continuing: &Block,
        counter: Handle<LocalVariable>,
    ) -> Result<i64, String> {
        let mut step = None;
        for statement in continuing.iter() {
            match statement {
                Statement::Emit(_) => {}
                Statement::Store { pointer, value }
                    if step.is_none()
                        && self.local(*pointer) == Some(counter) =>
                {
                    let Expression::Binary { op, left, right } =
                        self.function.expressions[*value]
                    else {
                        return Err(LOOPS.into());
                    };
                    let by = (self.loaded(left) == Some(counter))
                        .then(|| self.number(right))
                        .flatten()
                        .filter(|by| *by != 0)
                        .ok_or(LOOPS)?;
                    step = Some(match op {
                        BinaryOperator::Add => by,
                        BinaryOperator::Subtract => -by,
                        _ => return Err(LOOPS.into()),
                    });
                }
                _ => return Err(LOOPS.into()),
            }
        }
        step.ok_or_else(|| LOOPS.into())
    }

    /// The places in `block` that store to `counter`, or hand a
    /// pointer to it to a function.
    fn writes(
        &self,
        block: &[Statement],
        counter: Handle<LocalVariable>,
    ) -> usize {
        block
            .iter()
            .map(|statement| match statement {
                Statement::Store { pointer, .. } => {
                    usize::from(self.local(*pointer) == Some(counter))
                }
                Statement::Call { arguments, .. } => arguments
                    .iter()
                    .filter(|argument| {
                        self.local(**argument) == Some(counter)
                    })
                    .count(),
                Statement::Block(inner) => {
                    self.writes(inner, counter)
                }
                Statement::If { accept, reject, .. } => {
                    self.writes(accept, counter)
                        + self.writes(reject, counter)
                }
                Statement::Switch { cases, .. } => cases
                    .iter()
                    .map(|case| self.writes(&case.body, counter))
                    .sum(),
                Statement::Loop {
                    body, continuing, ..
                } => {
                    self.writes(body, counter)
                        + self.writes(continuing, counter)
                }
                _ => 0,
            })
            .sum()
    }

    /// The local variable `pointer` points at.
    fn local(
        &self,
        pointer: Handle<Expression>,
    ) -> Option<Handle<LocalVariable>> {
        match self.function.expressions[pointer] {
            Expression::LocalVariable(local) => Some(local),
            _ => None,
        }
    }

    /// The local variable `value` is a load of.
    fn loaded(
        &self,
        value: Handle<Expression>,
    ) -> Option<Handle<LocalVariable>> {
        match self.function.expressions[value] {
            Expression::Load { pointer } => self.local(pointer),
            _ => None,
        }
    }

    /// The whole number `value` is, if it is one written out or a
    /// constant.
    fn number(&self, value: Handle<Expression>) -> Option<i64> {
        let literal = match self.function.expressions[value] {
            Expression::Literal(literal) => literal,
            Expression::Constant(constant) => {
                let init = self.module.constants[constant].init;
                match self.module.global_expressions[init] {
                    Expression::Literal(literal) => literal,
                    _ => return None,
                }
            }
            _ => return None,
        };
        match literal {
            Literal::I32(n) => Some(n.into()),
            Literal::U32(n) => Some(n.into()),
            Literal::AbstractInt(n) => Some(n),
            _ => None,
        }
    }

    /// The least and the most a counter of `local`'s type holds.
    fn span(&self, local: &LocalVariable) -> Option<(i64, i64)> {
        match self.module.types[local.ty].inner {
            TypeInner::Scalar(Scalar {
                kind: ScalarKind::Sint,
                width: 4,
            }) => Some((i32::MIN.into(), i32::MAX.into())),
            TypeInner::Scalar(Scalar {
                kind: ScalarKind::Uint,
                width: 4,
            }) => Some((0, u32::MAX.into())),
            _ => None,
        }
    }
}

/// The times a counter from `start` crosses `distance` in steps of
/// `step`, and the value it ends the loop with.
fn over(distance: i64, step: i64, start: i64) -> (u64, i64) {
    let count = match distance > 0 {
        true => distance.unsigned_abs().div_ceil(step.unsigned_abs()),
        false => 0,
    };
    (count, start + count as i64 * step)
}

/// The comparison with its two sides swapped.
fn flipped(op: BinaryOperator) -> BinaryOperator {
    match op {
        BinaryOperator::Less => BinaryOperator::Greater,
        BinaryOperator::LessEqual => BinaryOperator::GreaterEqual,
        BinaryOperator::Greater => BinaryOperator::Less,
        BinaryOperator::GreaterEqual => BinaryOperator::LessEqual,
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(source: &str) -> Result<(), String> {
        checked("glow", source, &Limits::default()).map(|_| ())
    }

    fn shade(body: &str) -> String {
        format!(
            "fn shade(surface: Surface) -> vec4<f32> {{\n{body}\n}}"
        )
    }

    #[test]
    fn a_shader_of_the_interface_with_counted_loops_is_kept() {
        let ok = shade(
            "var sum = 0.0;
             for (var i = 0; i < 8; i++) {
                 for (var j = 4u; j > 0u; j -= 2u) { sum += 0.1; }
             }
             let wave = sin(surface.time + surface.position.x);
             return vec4<f32>(param(0u).rgb * wave * sum, 1.0);",
        );
        assert_eq!(check(&ok), Ok(()));
    }

    #[test]
    fn a_shader_that_could_run_on_or_reach_out_is_refused() {
        let refused = [
            shade("loop { } return vec4<f32>(1.0);"),
            shade(
                "var i = 0; while i < 4 { i++; } return \
                 vec4<f32>(1.0);",
            ),
            shade(
                "for (var i = 0; i < 4; i++) { i = 0; }
                 return vec4<f32>(1.0);",
            ),
            shade(
                "for (var i = 0u; i >= 0u; i--) { }
                 return vec4<f32>(1.0);",
            ),
            shade(
                "var i = 0;
                 for (var k = 0; k < 2; k++) {
                     for (; i < 8; i++) { }
                     i = -100000;
                 }
                 return vec4<f32>(1.0);",
            ),
            shade(
                "for (var i = 0; i < 64; i++) {
                     for (var j = 0; j < 64; j++) { }
                 }
                 return vec4<f32>(1.0);",
            ),
            format!(
                "@group(1) @binding(0) var<uniform> mine: \
                 vec4<f32>;\n{}",
                shade("return mine;")
            ),
            format!(
                "@compute @workgroup_size(1) fn spin() {{ }}\n{}",
                shade("return vec4<f32>(1.0);")
            ),
            shade(
                "#import bevy_pbr::mesh_view_bindings\nreturn \
                 vec4<f32>(1.0);",
            ),
            "fn shade(at: vec3<f32>) -> vec4<f32> { return \
             vec4<f32>(1.0); }"
                .into(),
            "x".repeat(Limits::default().shader_bytes + 1),
        ];
        for source in refused {
            assert!(check(&source).is_err(), "{source}");
        }
    }
}
