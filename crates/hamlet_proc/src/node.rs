//! Kinds of node: what each takes and gives, and how it runs.

use std::collections::BTreeMap;

use bevy_math::Vec3;

use crate::eval::{Problem, Run};
use crate::nodes;
use crate::value::{Value, ValueType};

/// A kind of node. Its names and words describe it to the editor, to
/// plugin authors and to agents.
#[derive(Clone, Copy, Debug)]
pub struct NodeKind {
    pub name: &'static str,
    pub doc: &'static str,
    pub inputs: &'static [PortSpec],
    pub outputs: &'static [PortSpec],
    pub run: fn(&mut Run) -> Result<(), Problem>,
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
}

/// The initial value of an input.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Initial {
    Number(f32),
    Bool(bool),
    Point(Vec3),
    Choice(&'static str),
}

impl From<Initial> for Value {
    fn from(initial: Initial) -> Self {
        match initial {
            Initial::Number(number) => Value::Number(number),
            Initial::Bool(flag) => Value::Bool(flag),
            Initial::Point(point) => Value::Point(point),
            Initial::Choice(choice) => {
                Value::Choice(choice.to_owned())
            }
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
    /// The built-in kinds.
    pub fn builtin() -> Self {
        let mut registry = Self::default();
        for (group, kinds) in nodes::KINDS {
            for kind in kinds {
                registry.add(group, *kind);
            }
        }
        registry
    }

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
