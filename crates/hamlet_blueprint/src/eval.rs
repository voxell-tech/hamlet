//! Running a ruleset.

use std::collections::{BTreeMap, BTreeSet};

use bevy_math::Vec3;
use hamlet_graph::{
    Asset, Catalogue, Does, Error, Graph, GraphError, Node, NodeId,
    NodeKind, Registry, Roll, Ruleset, Scope, Setting, Spread,
    ValueType, order, ports, validate,
};

use crate::field::Field;
use crate::kind;
use crate::seed::Seed;
use crate::value::{
    Curve, Frame, Object, ObjectId, TransformValue, Value,
};

/// The seeds a ruleset runs with, one for each scope above a
/// transform.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Seeds {
    pub realm: Seed,
    pub hamlet: Seed,
    pub item: Seed,
}

impl Seeds {
    fn of(&self, scope: Scope) -> Seed {
        match scope {
            Scope::Realm => self.realm,
            Scope::Hamlet => self.hamlet,
            Scope::Item | Scope::Object => self.item,
        }
    }
}

/// Runs `ruleset` from its entry graph with `inputs`, giving the
/// entry graph's outputs by name.
pub fn evaluate(
    registry: &Registry,
    ruleset: &Ruleset,
    seeds: Seeds,
    inputs: &BTreeMap<String, Value>,
) -> Result<BTreeMap<String, Value>, GraphError> {
    if let Some(error) =
        validate(registry, ruleset).into_iter().next()
    {
        return Err(error);
    }
    let mut runner = Runner {
        registry,
        ruleset,
        seeds,
        partial: None,
    };
    runner.run(&ruleset.entry, Seed::default(), inputs)
}

/// Runs `ruleset` as [`evaluate`] does, but a node that fails, as one
/// with an input left unset, only stops the nodes that need what it
/// would have made: the rest still run. Gives what was made, and the
/// error of each node that failed. A ruleset that does not validate
/// still gives its error alone.
pub fn evaluate_partly(
    registry: &Registry,
    ruleset: &Ruleset,
    seeds: Seeds,
    inputs: &BTreeMap<String, Value>,
) -> Result<(BTreeMap<String, Value>, Vec<GraphError>), GraphError> {
    if let Some(error) =
        validate(registry, ruleset).into_iter().next()
    {
        return Err(error);
    }
    let mut runner = Runner {
        registry,
        ruleset,
        seeds,
        partial: Some(Vec::new()),
    };
    let made = runner.run(&ruleset.entry, Seed::default(), inputs)?;
    let errors = runner.partial.unwrap_or_default();
    Ok((made, errors))
}

struct Runner<'a> {
    registry: &'a Registry,
    ruleset: &'a Ruleset,
    seeds: Seeds,
    /// The errors of the nodes that failed, for a run that goes on
    /// past them; none for a run that stops at the first.
    partial: Option<Vec<GraphError>>,
}

impl Runner<'_> {
    /// Runs the graph `name`. `path` names where it runs, so the same
    /// graph run from two nodes gives two sets of transform ids.
    fn run(
        &mut self,
        name: &str,
        path: Seed,
        given: &BTreeMap<String, Value>,
    ) -> Result<BTreeMap<String, Value>, GraphError> {
        let fail = |error| GraphError {
            graph: name.to_owned(),
            error,
        };
        let graph =
            self.ruleset.graphs.get(name).ok_or_else(|| {
                fail(Error::UnknownGraph(name.to_owned()))
            })?;
        // An input the graph above left unset varies, or takes its
        // default.
        let mut given = given.clone();
        for input in &graph.inputs {
            if given.contains_key(&input.name) {
                continue;
            }
            let value = match (input.vary, &input.default) {
                (Some(vary), _) => {
                    let key = self
                        .seeds
                        .of(vary.per)
                        .mix(path.0)
                        .mix_str(&input.name);
                    let picked = vary.low
                        + (vary.high - vary.low) * key.unit();
                    // A whole number varies between whole numbers.
                    if input.value_type == ValueType::Integer {
                        Value::Integer(picked.round() as i32)
                    } else {
                        Value::Number(picked)
                    }
                }
                (None, Some(default)) => default.clone().into(),
                (None, None) => continue,
            };
            given.insert(input.name.clone(), value);
        }
        let given = &given;
        let mut made: BTreeMap<(NodeId, String), Value> =
            BTreeMap::new();
        // The nodes that failed or need what one failed to make, in a
        // run that goes on past them.
        let mut skipped = BTreeSet::new();
        for index in order(graph).map_err(fail)? {
            let node = &graph.nodes[index];
            if graph.wires.iter().any(|wire| {
                wire.to.node == node.id
                    && skipped.contains(&wire.from.node)
            }) {
                skipped.insert(node.id);
                continue;
            }
            let outputs = match self
                .node(name, graph, node, path, &made, given)
            {
                Ok(outputs) => outputs,
                Err(error) => match &mut self.partial {
                    Some(errors) => {
                        errors.push(error);
                        skipped.insert(node.id);
                        continue;
                    }
                    None => return Err(error),
                },
            };
            for (port, value) in outputs {
                made.insert((node.id, port), value);
            }
        }
        graph
            .outputs
            .iter()
            .filter(|output| {
                self.partial.is_none()
                    || !skipped.contains(&output.from.node)
            })
            .map(|output| {
                made.get(&(
                    output.from.node,
                    output.from.name.clone(),
                ))
                .cloned()
                .map(|value| (output.name.clone(), value))
                .ok_or_else(|| {
                    fail(Error::Unset {
                        node: output.from.node,
                        port: output.from.name.clone(),
                    })
                })
            })
            .collect()
    }

    /// Runs one node of `graph`, giving what it makes by output.
    fn node(
        &mut self,
        name: &str,
        graph: &Graph,
        node: &Node,
        path: Seed,
        made: &BTreeMap<(NodeId, String), Value>,
        given: &BTreeMap<String, Value>,
    ) -> Result<BTreeMap<String, Value>, GraphError> {
        let fail = |error| GraphError {
            graph: name.to_owned(),
            error,
        };
        let seed = path.mix(u64::from(node.id.0));
        let (inputs, _) =
            ports(self.registry, self.ruleset, node).map_err(fail)?;
        let mut bound = BTreeMap::new();
        for port in &inputs {
            if let Some(binding) =
                bind(graph, node, &port.name, made, given)
                    .map_err(fail)?
            {
                bound.insert(port.name.clone(), binding);
            }
        }
        let outputs = match &node.does {
            Does::Kind(kind_name) => {
                let unknown = || {
                    fail(Error::UnknownKind {
                        node: node.id,
                        kind: kind_name.clone(),
                    })
                };
                let kind = self
                    .registry
                    .get(kind_name)
                    .ok_or_else(unknown)?;
                let runs =
                    kind::get(kind_name).ok_or_else(unknown)?;
                for spec in kind.inputs {
                    if let Some(initial) = spec.initial {
                        bound
                            .entry(spec.name.to_owned())
                            .or_insert_with(|| {
                                Binding::Value(initial.into())
                            });
                    }
                }
                let mut run = Run {
                    kind,
                    assets: &self.ruleset.assets,
                    node: node.id,
                    seed,
                    seeds: self.seeds,
                    inputs: bound,
                    outputs: BTreeMap::new(),
                };
                (runs.run)(&mut run).map_err(fail)?;
                // Only a kind's node puts down a new asset; a
                // graph's gives back what its own nodes did.
                for value in run.outputs.values() {
                    let Value::Objects(objects) = value else {
                        continue;
                    };
                    if let Some(unknown) =
                        objects.iter().find(|object| {
                            self.ruleset
                                .assets
                                .get(&object.asset)
                                .is_none()
                        })
                    {
                        return Err(fail(Error::UnknownAsset {
                            node: node.id,
                            asset: unknown.asset.clone(),
                        }));
                    }
                }
                run.outputs
            }
            Does::Graph(child) => {
                let mut child_given = BTreeMap::new();
                for (port, binding) in bound {
                    let value = match binding {
                        Binding::Value(value) => value,
                        Binding::Roll(roll) => {
                            let key = self
                                .seeds
                                .of(roll.scope)
                                .mix(seed.0)
                                .mix_str(&port);
                            roll_value(&roll.spread, key)
                        }
                    };
                    child_given.insert(port, value);
                }
                self.run(child, seed, &child_given)?
            }
        };
        Ok(outputs)
    }
}

/// How an input of a running node gets its value.
#[derive(Clone, Debug)]
enum Binding {
    Value(Value),
    Roll(Roll),
}

fn bind(
    graph: &Graph,
    node: &Node,
    port: &str,
    made: &BTreeMap<(NodeId, String), Value>,
    given: &BTreeMap<String, Value>,
) -> Result<Option<Binding>, Error> {
    if let Some(wire) = graph
        .wires
        .iter()
        .find(|wire| wire.to.node == node.id && wire.to.name == port)
    {
        let value = made
            .get(&(wire.from.node, wire.from.name.clone()))
            .ok_or_else(|| Error::Unset {
                node: wire.from.node,
                port: wire.from.name.clone(),
            })?;
        return Ok(Some(Binding::Value(value.clone())));
    }
    Ok(match node.settings.get(port) {
        Some(Setting::Fixed(constant)) => {
            Some(Binding::Value(constant.clone().into()))
        }
        Some(Setting::Random(roll)) => {
            Some(Binding::Roll(roll.clone()))
        }
        Some(Setting::FromParent(name)) => {
            let value = given
                .get(name)
                .ok_or_else(|| Error::NotGiven(name.clone()))?;
            Some(Binding::Value(value.clone()))
        }
        None => None,
    })
}

fn roll_value(spread: &Spread, key: Seed) -> Value {
    match spread {
        Spread::Range(low, high) => {
            Value::Number(low + (high - low) * key.unit())
        }
        Spread::Chance(chance) => Value::Bool(key.unit() < *chance),
        Spread::Weights(weights) => {
            Value::Choice(pick(weights, key).to_owned())
        }
    }
}

fn pick(weights: &[(String, f32)], key: Seed) -> &str {
    let total = weights
        .iter()
        .map(|(_, weight)| weight.max(0.0))
        .sum::<f32>();
    let mut left = key.unit() * total;
    for (choice, weight) in weights {
        left -= weight.max(0.0);
        if left < 0.0 {
            return choice;
        }
    }
    weights.last().map_or("", |(choice, _)| choice)
}

/// An object at the origin, for reading a field outside any object.
static ORIGIN: Object = Object {
    asset: String::new(),
    id: ObjectId::ROOT,
    frame: Frame::IDENTITY,
    index: 0,
    count: 1,
    along: 0.0,
    tags: Vec::new(),
};

/// A node of a built-in or added kind while it runs: its inputs to
/// read and its outputs to give.
pub struct Run<'a> {
    kind: &'a NodeKind,
    assets: &'a Catalogue,
    node: NodeId,
    seed: Seed,
    seeds: Seeds,
    inputs: BTreeMap<String, Binding>,
    outputs: BTreeMap<String, Value>,
}

impl Run<'_> {
    /// The most places or pieces one node makes from a number it is
    /// given: a 256 by 256 grid. Far more freezes the editor, which
    /// runs the graph after every edit.
    pub const MAX_COUNT: f32 = 65_536.0;

    fn binding(&self, port: &str) -> Result<&Binding, Error> {
        self.inputs.get(port).ok_or_else(|| Error::Unset {
            node: self.node,
            port: port.to_owned(),
        })
    }

    fn wrong(&self, port: &str, expected: ValueType) -> Error {
        let found = match self.inputs.get(port) {
            Some(Binding::Value(value)) => value.value_type(),
            Some(Binding::Roll(roll)) => roll.spread.value_type(),
            None => expected,
        };
        Error::WrongType {
            node: self.node,
            port: port.to_owned(),
            expected,
            found,
        }
    }

    /// Whether `port` has a wire, a setting or an initial value.
    pub fn is_set(&self, port: &str) -> bool {
        self.inputs.contains_key(port)
    }

    /// An error for the value of `port`.
    pub fn invalid(&self, port: &str, reason: &'static str) -> Error {
        Error::Invalid {
            node: self.node,
            port: port.to_owned(),
            reason,
        }
    }

    /// `wanted` places or pieces as a count, or an error on `port`
    /// when it is above [`Self::MAX_COUNT`].
    pub fn count(
        &self,
        port: &str,
        wanted: f32,
    ) -> Result<u32, Error> {
        if wanted > Self::MAX_COUNT {
            return Err(self.invalid(port, "would make too many"));
        }
        Ok(wanted as u32)
    }

    /// The seed of a roll of `port` at `scope`, at `at` when it is
    /// rolled per transform.
    fn roll_key(
        &self,
        port: &str,
        scope: Scope,
        at: Option<&Object>,
    ) -> Seed {
        let key = self.seeds.of(scope).mix(self.seed.0).mix_str(port);
        match (scope, at) {
            (Scope::Object, Some(transform)) => {
                key.mix(transform.id.0)
            }
            _ => key,
        }
    }

    fn rolled(
        &self,
        port: &str,
        roll: &Roll,
        at: Option<&Object>,
    ) -> Value {
        roll_value(&roll.spread, self.roll_key(port, roll.scope, at))
    }

    /// The number on `port`, at `at` when it is a field or rolled per
    /// transform.
    pub fn number(
        &self,
        port: &str,
        at: Option<&Object>,
    ) -> Result<f32, Error> {
        match self.binding(port)? {
            Binding::Value(Value::Number(number)) => Ok(*number),
            Binding::Value(Value::Integer(whole)) => {
                Ok(*whole as f32)
            }
            Binding::Value(Value::Field(field)) => {
                Ok(field.sample(at.unwrap_or(&ORIGIN)))
            }
            Binding::Roll(roll) => {
                match self.rolled(port, roll, at) {
                    Value::Number(number) => Ok(number),
                    _ => Err(self.wrong(port, ValueType::Number)),
                }
            }
            Binding::Value(_) => {
                Err(self.wrong(port, ValueType::Number))
            }
        }
    }

    /// The whole number on `port`, a number or field there rounded.
    pub fn integer(
        &self,
        port: &str,
        at: Option<&Object>,
    ) -> Result<i32, Error> {
        match self.binding(port)? {
            Binding::Value(Value::Integer(whole)) => Ok(*whole),
            _ => self
                .number(port, at)
                .map(|number| number.round() as i32)
                .map_err(|_| self.wrong(port, ValueType::Integer)),
        }
    }

    /// The bool on `port`, at `at` when it is a field or rolled per
    /// transform. A field is true above one half.
    pub fn flag(
        &self,
        port: &str,
        at: Option<&Object>,
    ) -> Result<bool, Error> {
        match self.binding(port)? {
            Binding::Value(Value::Bool(flag)) => Ok(*flag),
            Binding::Value(Value::Field(field)) => {
                Ok(field.sample(at.unwrap_or(&ORIGIN)) > 0.5)
            }
            Binding::Roll(roll) => {
                match self.rolled(port, roll, at) {
                    Value::Bool(flag) => Ok(flag),
                    _ => Err(self.wrong(port, ValueType::Bool)),
                }
            }
            Binding::Value(_) => {
                Err(self.wrong(port, ValueType::Bool))
            }
        }
    }

    /// The choice on `port`, checked against the kind's choices.
    pub fn choice(
        &self,
        port: &str,
        at: Option<&Object>,
    ) -> Result<String, Error> {
        let choice = match self.binding(port)? {
            Binding::Value(Value::Choice(choice)) => choice.clone(),
            Binding::Roll(roll) => {
                match self.rolled(port, roll, at) {
                    Value::Choice(choice) => choice,
                    _ => {
                        return Err(
                            self.wrong(port, ValueType::Choice)
                        );
                    }
                }
            }
            Binding::Value(_) => {
                return Err(self.wrong(port, ValueType::Choice));
            }
        };
        let allowed = self
            .kind
            .inputs
            .iter()
            .find(|spec| spec.name == port)
            .map_or(&[][..], |spec| spec.choices);
        if !allowed.is_empty() && !allowed.contains(&choice.as_str())
        {
            return Err(
                self.invalid(port, "is not one of its choices")
            );
        }
        Ok(choice)
    }

    pub fn point(&self, port: &str) -> Result<Vec3, Error> {
        self.point_at(port, None)
    }

    /// The point on `port` as the fields its axes are read from, a
    /// fixed point's as constants.
    pub fn point_fields(
        &self,
        port: &str,
    ) -> Result<[Field; 3], Error> {
        match self.binding(port)? {
            Binding::Value(Value::Point(point)) => {
                Ok(point.to_array().map(Field::Constant))
            }
            Binding::Value(Value::PointField(axes)) => {
                Ok((**axes).clone())
            }
            _ => Err(self.wrong(port, ValueType::Point)),
        }
    }

    /// The object at the origin, to read a field at outside any
    /// object.
    pub fn origin(&self) -> &Object {
        &ORIGIN
    }

    /// The point on `port`, at `at` when its axes are fields.
    pub fn point_at(
        &self,
        port: &str,
        at: Option<&Object>,
    ) -> Result<Vec3, Error> {
        match self.binding(port)? {
            Binding::Value(Value::Point(point)) => Ok(*point),
            Binding::Value(Value::PointField(axes)) => {
                Ok(Vec3::from(
                    axes.each_ref().map(|axis| {
                        axis.sample(at.unwrap_or(&ORIGIN))
                    }),
                ))
            }
            _ => Err(self.wrong(port, ValueType::Point)),
        }
    }

    pub fn curve(&self, port: &str) -> Result<&Curve, Error> {
        match self.binding(port)? {
            Binding::Value(Value::Curve(curve)) => Ok(curve),
            _ => Err(self.wrong(port, ValueType::Curve)),
        }
    }

    /// The transform on `port`, or none moving nothing when it is
    /// unset.
    pub fn transform(
        &self,
        port: &str,
    ) -> Result<TransformValue, Error> {
        if !self.is_set(port) {
            return Ok(TransformValue::IDENTITY);
        }
        match self.binding(port)? {
            Binding::Value(Value::Transform(transform)) => {
                Ok((**transform).clone())
            }
            _ => Err(self.wrong(port, ValueType::Transform)),
        }
    }

    pub fn objects(&self, port: &str) -> Result<&[Object], Error> {
        match self.binding(port)? {
            Binding::Value(Value::Objects(objects)) => Ok(objects),
            _ => Err(self.wrong(port, ValueType::Objects)),
        }
    }

    /// The field on `port`. A number is the same value everywhere.
    pub fn field(&self, port: &str) -> Result<Field, Error> {
        match self.binding(port)? {
            Binding::Value(Value::Field(field)) => Ok(field.clone()),
            Binding::Value(Value::Number(number)) => {
                Ok(Field::Constant(*number))
            }
            Binding::Value(Value::Integer(whole)) => {
                Ok(Field::Constant(*whole as f32))
            }
            _ => Err(self.wrong(port, ValueType::Field)),
        }
    }

    /// The asset named `name`, as the ruleset's catalogue has it.
    pub fn asset(&self, name: &str) -> Option<Asset> {
        self.assets.get(name)
    }

    /// The id of the `index`th object this node makes from `parent`.
    pub fn id(
        &self,
        parent: ObjectId,
        index: impl Into<u64>,
    ) -> ObjectId {
        ObjectId(self.seed.mix(parent.0).mix(index.into()).0)
    }

    /// A seed of this node that changes as often as `scope` does.
    pub fn chance(&self, scope: Scope) -> Seed {
        self.seeds.of(scope).mix(self.seed.0)
    }

    pub fn put(&mut self, port: &str, value: Value) {
        self.outputs.insert(port.to_owned(), value);
    }
}
