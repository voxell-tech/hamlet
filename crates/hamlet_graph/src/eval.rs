//! Checking a ruleset and running it.

use std::collections::{BTreeMap, BTreeSet};

use bevy_math::Vec3;

use crate::field::Field;
use crate::graph::{
    Does, Graph, Node, NodeId, Roll, Ruleset, Scope, Setting, Spread,
};
use crate::node::{NodeKind, Registry};
use crate::seed::Seed;
use crate::value::{
    Curve, Frame, Instance, Placement, PlacementId, Value, ValueType,
};

/// The seeds a ruleset runs with, one for each scope above a
/// placement.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Seeds {
    pub realm: Seed,
    pub hamlet: Seed,
    pub house: Seed,
}

impl Seeds {
    fn of(&self, scope: Scope) -> Seed {
        match scope {
            Scope::Realm => self.realm,
            Scope::Hamlet => self.hamlet,
            Scope::House | Scope::Placement => self.house,
        }
    }
}

/// Something wrong in a graph, found by [`validate`] or while running
/// it.
#[derive(Clone, PartialEq, Debug, thiserror::Error)]
pub enum Problem {
    #[error("no graph is named {0}")]
    UnknownGraph(String),
    #[error("{node} is of an unknown kind, {kind}")]
    UnknownKind { node: NodeId, kind: String },
    #[error("no node is {0}")]
    UnknownNode(NodeId),
    #[error("two nodes are {0}")]
    SameId(NodeId),
    #[error("{node} has no port {port}")]
    UnknownPort { node: NodeId, port: String },
    #[error("the graph has no input {0}")]
    UnknownInput(String),
    #[error("{node}.{port} takes {expected:?}, not {found:?}")]
    WrongType {
        node: NodeId,
        port: String,
        expected: ValueType,
        found: ValueType,
    },
    #[error("{node}.{port} has two wires")]
    TwoWires { node: NodeId, port: String },
    #[error("{node}.{port} is not set")]
    Unset { node: NodeId, port: String },
    #[error("{node}.{port} {reason}")]
    Invalid {
        node: NodeId,
        port: String,
        reason: &'static str,
    },
    #[error("the graph input {0} is not given")]
    NotGiven(String),
    #[error("the wires go round in a loop through {0:?}")]
    Loop(Vec<NodeId>),
    #[error("the graph runs itself")]
    RunsItself,
}

/// A [`Problem`] and the graph it is in.
#[derive(Clone, PartialEq, Debug, thiserror::Error)]
#[error("{graph}: {problem}")]
pub struct GraphProblem {
    pub graph: String,
    pub problem: Problem,
}

/// A port's name and type.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PortInfo {
    pub name: String,
    pub value_type: ValueType,
}

/// The inputs and outputs of `node`, from its kind or the graph it
/// runs.
pub fn ports(
    registry: &Registry,
    ruleset: &Ruleset,
    node: &Node,
) -> Result<(Vec<PortInfo>, Vec<PortInfo>), Problem> {
    match &node.does {
        Does::Kind(name) => {
            let kind = registry.get(name).ok_or_else(|| {
                Problem::UnknownKind {
                    node: node.id,
                    kind: name.clone(),
                }
            })?;
            let info = |specs: &[crate::node::PortSpec]| {
                specs
                    .iter()
                    .map(|spec| PortInfo {
                        name: spec.name.to_owned(),
                        value_type: spec.value_type,
                    })
                    .collect()
            };
            Ok((info(kind.inputs), info(kind.outputs)))
        }
        Does::Graph(name) => {
            let graph = ruleset
                .graphs
                .get(name)
                .ok_or_else(|| Problem::UnknownGraph(name.clone()))?;
            let inputs = graph
                .inputs
                .iter()
                .map(|input| PortInfo {
                    name: input.name.clone(),
                    value_type: input.value_type,
                })
                .collect();
            let outputs = graph
                .outputs
                .iter()
                .map(|output| PortInfo {
                    name: output.name.clone(),
                    value_type: output.value_type,
                })
                .collect();
            Ok((inputs, outputs))
        }
    }
}

/// Every problem in `ruleset` that can be found without running it.
pub fn validate(
    registry: &Registry,
    ruleset: &Ruleset,
) -> Vec<GraphProblem> {
    let mut problems = Vec::new();
    if !ruleset.graphs.contains_key(&ruleset.entry) {
        problems.push(GraphProblem {
            graph: ruleset.entry.clone(),
            problem: Problem::UnknownGraph(ruleset.entry.clone()),
        });
    }
    for (name, graph) in &ruleset.graphs {
        let mut found = check(registry, ruleset, graph);
        if runs_itself(ruleset, name, &mut Vec::new()) {
            found.push(Problem::RunsItself);
        }
        problems.extend(found.into_iter().map(|problem| {
            GraphProblem {
                graph: name.clone(),
                problem,
            }
        }));
    }
    problems
}

fn check(
    registry: &Registry,
    ruleset: &Ruleset,
    graph: &Graph,
) -> Vec<Problem> {
    let mut problems = Vec::new();
    let mut ids = BTreeSet::new();
    let mut ports_of = BTreeMap::new();
    for node in &graph.nodes {
        if !ids.insert(node.id) {
            problems.push(Problem::SameId(node.id));
        }
        match ports(registry, ruleset, node) {
            Ok(found) => {
                ports_of.insert(node.id, found);
            }
            Err(problem) => problems.push(problem),
        }
    }
    let port_type = |node: NodeId, name: &str, output: bool| {
        ports_of.get(&node).and_then(|(inputs, outputs)| {
            let list: &Vec<PortInfo> =
                if output { outputs } else { inputs };
            list.iter()
                .find(|port| port.name == name)
                .map(|port| port.value_type)
        })
    };
    for node in &graph.nodes {
        let choices = match &node.does {
            Does::Kind(name) => {
                registry.get(name).map(|kind| kind.inputs)
            }
            Does::Graph(_) => None,
        };
        for (port, setting) in &node.settings {
            let Some(expected) = port_type(node.id, port, false)
            else {
                if ports_of.contains_key(&node.id) {
                    problems.push(Problem::UnknownPort {
                        node: node.id,
                        port: port.clone(),
                    });
                }
                continue;
            };
            let found = match setting {
                Setting::Fixed(constant) => {
                    let allowed = choices
                        .and_then(|inputs| {
                            inputs
                                .iter()
                                .find(|spec| spec.name == port)
                        })
                        .map_or(&[][..], |spec| spec.choices);
                    if let crate::value::Constant::Choice(choice) =
                        constant
                        && !allowed.is_empty()
                        && !allowed.contains(&choice.as_str())
                    {
                        problems.push(Problem::Invalid {
                            node: node.id,
                            port: port.clone(),
                            reason: "is not one of its choices",
                        });
                    }
                    Some(constant.value_type())
                }
                Setting::Random(roll) => {
                    Some(roll.spread.value_type())
                }
                Setting::FromParent(name) => {
                    let input = graph
                        .inputs
                        .iter()
                        .find(|input| &input.name == name);
                    if input.is_none() {
                        problems.push(Problem::UnknownInput(
                            name.clone(),
                        ));
                    }
                    input.map(|input| input.value_type)
                }
            };
            if let Some(found) = found
                && !expected.accepts(found)
            {
                problems.push(Problem::WrongType {
                    node: node.id,
                    port: port.clone(),
                    expected,
                    found,
                });
            }
        }
    }
    let mut wired = BTreeSet::new();
    for wire in &graph.wires {
        for end in [&wire.from, &wire.to] {
            if !ids.contains(&end.node) {
                problems.push(Problem::UnknownNode(end.node));
            }
        }
        let from = port_type(wire.from.node, &wire.from.name, true);
        let to = port_type(wire.to.node, &wire.to.name, false);
        for (end, found) in [(&wire.from, from), (&wire.to, to)] {
            if found.is_none() && ports_of.contains_key(&end.node) {
                problems.push(Problem::UnknownPort {
                    node: end.node,
                    port: end.name.clone(),
                });
            }
        }
        if let (Some(from), Some(to)) = (from, to)
            && !to.accepts(from)
        {
            problems.push(Problem::WrongType {
                node: wire.to.node,
                port: wire.to.name.clone(),
                expected: to,
                found: from,
            });
        }
        if !wired.insert((wire.to.node, wire.to.name.as_str())) {
            problems.push(Problem::TwoWires {
                node: wire.to.node,
                port: wire.to.name.clone(),
            });
        }
    }
    for output in &graph.outputs {
        match port_type(output.from.node, &output.from.name, true) {
            Some(found) if !output.value_type.accepts(found) => {
                problems.push(Problem::WrongType {
                    node: output.from.node,
                    port: output.from.name.clone(),
                    expected: output.value_type,
                    found,
                });
            }
            Some(_) => {}
            None => problems.push(Problem::UnknownPort {
                node: output.from.node,
                port: output.from.name.clone(),
            }),
        }
    }
    if let Err(problem) = order(graph) {
        problems.push(problem);
    }
    problems
}

fn runs_itself(
    ruleset: &Ruleset,
    name: &str,
    running: &mut Vec<String>,
) -> bool {
    if running.iter().any(|above| above == name) {
        return true;
    }
    let Some(graph) = ruleset.graphs.get(name) else {
        return false;
    };
    running.push(name.to_owned());
    let found = graph.nodes.iter().any(|node| match &node.does {
        Does::Graph(child) => runs_itself(ruleset, child, running),
        Does::Kind(_) => false,
    });
    running.pop();
    found
}

/// The nodes in an order where each comes after those wired into it.
fn order(graph: &Graph) -> Result<Vec<usize>, Problem> {
    let index: BTreeMap<NodeId, usize> = graph
        .nodes
        .iter()
        .enumerate()
        .map(|(i, node)| (node.id, i))
        .collect();
    let mut waiting = vec![0usize; graph.nodes.len()];
    for wire in &graph.wires {
        if let (Some(_), Some(&to)) =
            (index.get(&wire.from.node), index.get(&wire.to.node))
        {
            waiting[to] += 1;
        }
    }
    let mut ready = (0..graph.nodes.len())
        .filter(|&i| waiting[i] == 0)
        .collect::<Vec<_>>();
    let mut sorted = Vec::with_capacity(graph.nodes.len());
    while let Some(i) = ready.pop() {
        sorted.push(i);
        for wire in graph
            .wires
            .iter()
            .filter(|wire| wire.from.node == graph.nodes[i].id)
        {
            if let Some(&to) = index.get(&wire.to.node) {
                waiting[to] -= 1;
                if waiting[to] == 0 {
                    ready.push(to);
                }
            }
        }
    }
    if sorted.len() < graph.nodes.len() {
        let stuck = (0..graph.nodes.len())
            .filter(|&i| waiting[i] > 0)
            .map(|i| graph.nodes[i].id)
            .collect();
        return Err(Problem::Loop(stuck));
    }
    Ok(sorted)
}

/// Runs `ruleset` from its entry graph with `inputs`, giving the
/// entry graph's outputs by name.
pub fn evaluate(
    registry: &Registry,
    ruleset: &Ruleset,
    seeds: Seeds,
    inputs: &BTreeMap<String, Value>,
) -> Result<BTreeMap<String, Value>, GraphProblem> {
    if let Some(problem) =
        validate(registry, ruleset).into_iter().next()
    {
        return Err(problem);
    }
    let runner = Runner {
        registry,
        ruleset,
        seeds,
    };
    runner.run(&ruleset.entry, Seed::default(), inputs)
}

struct Runner<'a> {
    registry: &'a Registry,
    ruleset: &'a Ruleset,
    seeds: Seeds,
}

impl Runner<'_> {
    /// Runs the graph `name`. `path` names where it runs, so the same
    /// graph run from two nodes gives two sets of placement ids.
    fn run(
        &self,
        name: &str,
        path: Seed,
        given: &BTreeMap<String, Value>,
    ) -> Result<BTreeMap<String, Value>, GraphProblem> {
        let fail = |problem| GraphProblem {
            graph: name.to_owned(),
            problem,
        };
        let graph =
            self.ruleset.graphs.get(name).ok_or_else(|| {
                fail(Problem::UnknownGraph(name.to_owned()))
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
                    Value::Number(
                        vary.low
                            + (vary.high - vary.low) * key.unit(),
                    )
                }
                (None, Some(default)) => default.clone().into(),
                (None, None) => continue,
            };
            given.insert(input.name.clone(), value);
        }
        let given = &given;
        let mut made: BTreeMap<(NodeId, String), Value> =
            BTreeMap::new();
        for index in order(graph).map_err(fail)? {
            let node = &graph.nodes[index];
            let seed = path.mix(u64::from(node.id.0));
            let (inputs, _) =
                ports(self.registry, self.ruleset, node)
                    .map_err(fail)?;
            let mut bound = BTreeMap::new();
            for port in &inputs {
                if let Some(binding) =
                    bind(graph, node, &port.name, &made, given)
                        .map_err(fail)?
                {
                    bound.insert(port.name.clone(), binding);
                }
            }
            let outputs = match &node.does {
                Does::Kind(kind_name) => {
                    let Some(kind) = self.registry.get(kind_name)
                    else {
                        return Err(fail(Problem::UnknownKind {
                            node: node.id,
                            kind: kind_name.clone(),
                        }));
                    };
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
                        node: node.id,
                        seed,
                        seeds: self.seeds,
                        inputs: bound,
                        outputs: BTreeMap::new(),
                    };
                    (kind.run)(&mut run).map_err(fail)?;
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
            for (port, value) in outputs {
                made.insert((node.id, port), value);
            }
        }
        graph
            .outputs
            .iter()
            .map(|output| {
                made.get(&(
                    output.from.node,
                    output.from.name.clone(),
                ))
                .cloned()
                .map(|value| (output.name.clone(), value))
                .ok_or_else(|| {
                    fail(Problem::Unset {
                        node: output.from.node,
                        port: output.from.name.clone(),
                    })
                })
            })
            .collect()
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
) -> Result<Option<Binding>, Problem> {
    if let Some(wire) = graph
        .wires
        .iter()
        .find(|wire| wire.to.node == node.id && wire.to.name == port)
    {
        let value = made
            .get(&(wire.from.node, wire.from.name.clone()))
            .ok_or_else(|| Problem::Unset {
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
                .ok_or_else(|| Problem::NotGiven(name.clone()))?;
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

/// A placement at the origin, for reading a field outside any
/// placement.
const ORIGIN: Placement = Placement {
    id: PlacementId::ROOT,
    frame: Frame::IDENTITY,
    size: Vec3::ONE,
    index: 0,
    count: 1,
    along: 0.0,
    tags: Vec::new(),
};

/// A node of a built-in or added kind while it runs: its inputs to
/// read and its outputs to give.
pub struct Run<'a> {
    kind: &'a NodeKind,
    node: NodeId,
    seed: Seed,
    seeds: Seeds,
    inputs: BTreeMap<String, Binding>,
    outputs: BTreeMap<String, Value>,
}

impl Run<'_> {
    fn binding(&self, port: &str) -> Result<&Binding, Problem> {
        self.inputs.get(port).ok_or_else(|| Problem::Unset {
            node: self.node,
            port: port.to_owned(),
        })
    }

    fn wrong(&self, port: &str, expected: ValueType) -> Problem {
        let found = match self.inputs.get(port) {
            Some(Binding::Value(value)) => value.value_type(),
            Some(Binding::Roll(roll)) => roll.spread.value_type(),
            None => expected,
        };
        Problem::WrongType {
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

    /// A problem with the value of `port`.
    pub fn invalid(
        &self,
        port: &str,
        reason: &'static str,
    ) -> Problem {
        Problem::Invalid {
            node: self.node,
            port: port.to_owned(),
            reason,
        }
    }

    /// The seed of a roll of `port` at `scope`, at `at` when it is
    /// rolled per placement.
    fn roll_key(
        &self,
        port: &str,
        scope: Scope,
        at: Option<&Placement>,
    ) -> Seed {
        let key = self.seeds.of(scope).mix(self.seed.0).mix_str(port);
        match (scope, at) {
            (Scope::Placement, Some(placement)) => {
                key.mix(placement.id.0)
            }
            _ => key,
        }
    }

    fn rolled(
        &self,
        port: &str,
        roll: &Roll,
        at: Option<&Placement>,
    ) -> Value {
        roll_value(&roll.spread, self.roll_key(port, roll.scope, at))
    }

    /// The number on `port`, at `at` when it is a field or rolled per
    /// placement.
    pub fn number(
        &self,
        port: &str,
        at: Option<&Placement>,
    ) -> Result<f32, Problem> {
        match self.binding(port)? {
            Binding::Value(Value::Number(number)) => Ok(*number),
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

    /// The bool on `port`, at `at` when it is a field or rolled per
    /// placement. A field is true above one half.
    pub fn flag(
        &self,
        port: &str,
        at: Option<&Placement>,
    ) -> Result<bool, Problem> {
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
        at: Option<&Placement>,
    ) -> Result<String, Problem> {
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

    pub fn point(&self, port: &str) -> Result<Vec3, Problem> {
        match self.binding(port)? {
            Binding::Value(Value::Point(point)) => Ok(*point),
            _ => Err(self.wrong(port, ValueType::Point)),
        }
    }

    pub fn curve(&self, port: &str) -> Result<&Curve, Problem> {
        match self.binding(port)? {
            Binding::Value(Value::Curve(curve)) => Ok(curve),
            _ => Err(self.wrong(port, ValueType::Curve)),
        }
    }

    pub fn placements(
        &self,
        port: &str,
    ) -> Result<&[Placement], Problem> {
        match self.binding(port)? {
            Binding::Value(Value::Placements(placements)) => {
                Ok(placements)
            }
            _ => Err(self.wrong(port, ValueType::Placements)),
        }
    }

    pub fn geometry(
        &self,
        port: &str,
    ) -> Result<&[Instance], Problem> {
        match self.binding(port)? {
            Binding::Value(Value::Geometry(instances)) => {
                Ok(instances)
            }
            _ => Err(self.wrong(port, ValueType::Geometry)),
        }
    }

    /// The field on `port`. A number is the same value everywhere.
    pub fn field(&self, port: &str) -> Result<Field, Problem> {
        match self.binding(port)? {
            Binding::Value(Value::Field(field)) => Ok(field.clone()),
            Binding::Value(Value::Number(number)) => {
                Ok(Field::Constant(*number))
            }
            _ => Err(self.wrong(port, ValueType::Field)),
        }
    }

    /// The id of the `index`th placement this node makes from
    /// `parent`.
    pub fn id(&self, parent: PlacementId, index: u32) -> PlacementId {
        PlacementId(self.seed.mix(parent.0).mix(u64::from(index)).0)
    }

    /// A seed of this node that changes as often as `scope` does.
    pub fn chance(&self, scope: Scope) -> Seed {
        self.seeds.of(scope).mix(self.seed.0)
    }

    pub fn put(&mut self, port: &str, value: Value) {
        self.outputs.insert(port.to_owned(), value);
    }
}
