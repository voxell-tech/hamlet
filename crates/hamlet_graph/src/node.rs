//! Kinds of node: what each takes and gives.

use std::collections::BTreeMap;

use bevy_math::{Vec3, Vec4};

use crate::value::{Constant, PathPoint, ValueType};

/// A kind of node. Its names and words describe it to the editor, to
/// plugin authors and to agents.
#[derive(Clone, Copy, Debug)]
pub struct NodeKind {
    pub name: &'static str,
    pub doc: &'static str,
    pub inputs: &'static [PortSpec],
    pub outputs: &'static [PortSpec],
}

/// A port of a [`NodeKind`].
#[derive(Clone, Copy, Debug)]
pub struct PortSpec {
    pub name: &'static str,
    pub value_type: ValueType,
    pub doc: &'static str,
    /// The value of an input with no wire and no setting.
    pub initial: Option<Initial>,
    /// The choices a choice input takes. Empty for any text.
    pub choices: &'static [&'static str],
    /// What of the catalogue this choice input names, its choices
    /// those the catalogue has.
    pub listed: Option<Listed>,
    /// Whether this choice input names its node: an editor shows
    /// each choice as a node of its own, titled by it.
    pub names: bool,
    /// The choices of the input that names the node this input is
    /// used with. Empty for every one.
    pub used_with: &'static [&'static str],
    /// Whether this input repeats: one port per wire, numbered from
    /// one, and an unwired one after them.
    pub grows: bool,
}

impl PortSpec {
    pub const fn new(
        name: &'static str,
        value_type: ValueType,
        doc: &'static str,
    ) -> Self {
        Self {
            name,
            value_type,
            doc,
            initial: None,
            choices: &[],
            listed: None,
            names: false,
            used_with: &[],
            grows: false,
        }
    }

    pub const fn initial(self, initial: Initial) -> Self {
        Self {
            initial: Some(initial),
            ..self
        }
    }

    pub const fn choices(
        self,
        choices: &'static [&'static str],
    ) -> Self {
        Self { choices, ..self }
    }

    pub const fn listed(self, listed: Listed) -> Self {
        Self {
            listed: Some(listed),
            ..self
        }
    }

    pub const fn names(self) -> Self {
        Self {
            names: true,
            ..self
        }
    }

    pub const fn used_with(
        self,
        used_with: &'static [&'static str],
    ) -> Self {
        Self { used_with, ..self }
    }

    pub const fn grows(self) -> Self {
        Self {
            grows: true,
            ..self
        }
    }

    /// The name of this growing input's port numbered `number`.
    pub fn port(&self, number: usize) -> String {
        format!("{}_{number}", self.name)
    }

    /// The number of `port` among this growing input's ports.
    pub fn number_of(&self, port: &str) -> Option<usize> {
        if !self.grows {
            return None;
        }
        let number = port
            .strip_prefix(self.name)?
            .strip_prefix('_')?
            .parse::<usize>()
            .ok()?;
        (number > 0).then_some(number)
    }

    /// Whether `port` is this input, or one of its ports if it grows.
    pub fn is_for(&self, port: &str) -> bool {
        if self.grows {
            self.number_of(port).is_some()
        } else {
            self.name == port
        }
    }
}

impl NodeKind {
    /// The input that `port` is, or is one port of.
    pub fn input(&self, port: &str) -> Option<&'static PortSpec> {
        self.inputs.iter().find(|spec| spec.is_for(port))
    }
}

/// What of the catalogue a choice input names.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Listed {
    Assets,
    /// A socket of any asset.
    Sockets,
}

/// The initial value of an input.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Initial {
    Number(f32),
    Integer(i32),
    Bool(bool),
    Point(Vec3),
    Choice(&'static str),
    Color(Vec4),
    Path(&'static [PathPoint]),
}

impl From<Initial> for Constant {
    fn from(initial: Initial) -> Self {
        match initial {
            Initial::Number(number) => Constant::Number(number),
            Initial::Integer(whole) => Constant::Integer(whole),
            Initial::Bool(flag) => Constant::Bool(flag),
            Initial::Point(point) => Constant::Point(point),
            Initial::Choice(choice) => {
                Constant::Choice(choice.to_owned())
            }
            Initial::Color(color) => Constant::Color(color),
            Initial::Path(points) => Constant::Path(points.to_vec()),
        }
    }
}

/// The kinds of node a ruleset may use, by name, each in a group it
/// is listed under.
#[derive(Clone, Debug, Default)]
pub struct Registry {
    kinds: BTreeMap<&'static str, (&'static str, NodeKind)>,
}

impl Registry {
    /// Adds `kind` to `group`, in place of one of the same name.
    pub fn add(&mut self, group: &'static str, kind: NodeKind) {
        self.kinds.insert(kind.name, (group, kind));
    }

    pub fn get(&self, name: &str) -> Option<&NodeKind> {
        self.kinds.get(name).map(|(_, kind)| kind)
    }

    /// Every kind with its group, by name.
    pub fn kinds(
        &self,
    ) -> impl Iterator<Item = (&'static str, &NodeKind)> {
        self.kinds.values().map(|(group, kind)| (*group, kind))
    }
}
