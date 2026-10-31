//! Graphs as their files hold them.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use bevy_math::Vec2;
use serde::{Deserialize, Serialize};

use crate::catalogue::Catalogue;
use crate::value::{Constant, ValueType};

/// An id of a node, unique in its graph and kept in the file, so
/// seeds hold when nodes are moved or added.
#[derive(
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Debug,
    Default,
    Serialize,
    Deserialize,
)]
#[serde(transparent)]
pub struct NodeId(pub u32);

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// What a node runs.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Does {
    /// A kind from the [`Registry`](crate::Registry), by name.
    Kind(String),
    /// Another graph of the ruleset, by name.
    Graph(String),
}

/// A step of a graph, and how its unwired inputs are set.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub does: Does,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub settings: BTreeMap<String, Setting>,
}

/// How an unwired input gets its value. An input with no setting
/// takes its kind's initial value.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub enum Setting {
    Fixed(Constant),
    Random(Roll),
    /// An input of the graph, set by the graph above or the game.
    FromParent(String),
}

/// A random value and how often it is rolled again.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Roll {
    pub spread: Spread,
    pub scope: Scope,
}

/// What a [`Roll`] picks from.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub enum Spread {
    /// A number between the two, the first included.
    Range(f32, f32),
    /// True with this chance, from 0 to 1.
    Chance(f32),
    /// A choice, each as likely as its weight.
    Weights(Vec<(String, f32)>),
}

impl Spread {
    pub fn value_type(&self) -> ValueType {
        match self {
            Spread::Range(..) => ValueType::Number,
            Spread::Chance(_) => ValueType::Bool,
            Spread::Weights(_) => ValueType::Choice,
        }
    }
}

/// How often a random value or a noise's seed changes.
#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize,
)]
pub enum Scope {
    Realm,
    Hamlet,
    House,
    Placement,
}

/// One end of a wire: a node's port by name.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Port {
    pub node: NodeId,
    pub name: String,
}

/// An output joined to an input.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Wire {
    pub from: Port,
    pub to: Port,
}

/// An input of a graph, set by the graph above or the game.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct GraphInput {
    pub name: String,
    pub value_type: ValueType,
    /// What it is when the graph above does not set it, unless it
    /// varies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Constant>,
    /// How a number input is picked when the graph above does not
    /// set it. It takes its default when it does not vary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vary: Option<Vary>,
}

/// A number picked between `low` and `high`, again for each house,
/// hamlet or realm.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct Vary {
    pub low: f32,
    pub high: f32,
    pub per: Scope,
}

/// An output of a graph: what a node's port gives.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct GraphOutput {
    pub name: String,
    pub value_type: ValueType,
    pub from: Port,
}

/// Nodes joined by wires, with named inputs and outputs.
#[derive(
    Clone, PartialEq, Debug, Default, Serialize, Deserialize,
)]
pub struct Graph {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<GraphInput>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<GraphOutput>,
    pub nodes: Vec<Node>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub wires: Vec<Wire>,
}

impl Graph {
    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.iter().find(|node| node.id == id)
    }

    /// An id no node has yet.
    pub fn next_id(&self) -> NodeId {
        NodeId(
            self.nodes
                .iter()
                .map(|node| node.id.0 + 1)
                .max()
                .unwrap_or(1),
        )
    }
}

/// A graph as the editor keeps it: the graph, where each node is
/// drawn, and the groups its nodes are framed in.
#[derive(
    Clone, PartialEq, Debug, Default, Serialize, Deserialize,
)]
pub struct Sheet {
    pub graph: Graph,
    #[serde(default)]
    pub layout: BTreeMap<NodeId, Vec2>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<Group>,
}

/// Nodes framed together under a title, for the one reading the
/// sheet. A node is in one group at most.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Group {
    pub title: String,
    pub nodes: BTreeSet<NodeId>,
}

impl Sheet {
    pub fn to_ron(&self) -> Result<String, ron::Error> {
        to_ron(self)
    }

    pub fn from_ron(
        text: &str,
    ) -> Result<Self, ron::error::SpannedError> {
        ron::from_str(text)
    }
}

/// What the game runs: graphs by name, without the editor's layout,
/// the one that makes a house, and the assets they add.
#[derive(
    Clone, PartialEq, Debug, Default, Serialize, Deserialize,
)]
pub struct Ruleset {
    pub entry: String,
    pub graphs: BTreeMap<String, Graph>,
    #[serde(default, skip_serializing_if = "Catalogue::is_empty")]
    pub assets: Catalogue,
}

impl Ruleset {
    /// The ruleset of `sheets`, run from the one named `entry`.
    pub fn export(
        entry: &str,
        sheets: &BTreeMap<String, Sheet>,
        assets: &Catalogue,
    ) -> Self {
        Self {
            entry: entry.to_owned(),
            graphs: sheets
                .iter()
                .map(|(name, sheet)| {
                    (name.clone(), sheet.graph.clone())
                })
                .collect(),
            assets: assets.clone(),
        }
    }

    pub fn to_ron(&self) -> Result<String, ron::Error> {
        to_ron(self)
    }

    pub fn from_ron(
        text: &str,
    ) -> Result<Self, ron::error::SpannedError> {
        ron::from_str(text)
    }
}

/// The sheets an author edits together, by name: a graph and those it
/// uses as nodes. The one named `entry` makes the thing. The assets
/// the book adds are its, shared by every sheet.
#[derive(
    Clone, PartialEq, Debug, Default, Serialize, Deserialize,
)]
pub struct Book {
    pub entry: String,
    pub sheets: BTreeMap<String, Sheet>,
    #[serde(default, skip_serializing_if = "Catalogue::is_empty")]
    pub assets: Catalogue,
}

impl Book {
    /// A book of one sheet, named `name`.
    pub fn of(name: &str, sheet: Sheet) -> Self {
        Self {
            entry: name.to_owned(),
            sheets: BTreeMap::from([(name.to_owned(), sheet)]),
            assets: Catalogue::default(),
        }
    }

    /// The ruleset the game runs from this book.
    pub fn export(&self) -> Ruleset {
        Ruleset::export(&self.entry, &self.sheets, &self.assets)
    }

    pub fn to_ron(&self) -> Result<String, ron::Error> {
        to_ron(self)
    }

    pub fn from_ron(
        text: &str,
    ) -> Result<Self, ron::error::SpannedError> {
        ron::from_str(text)
    }
}

fn to_ron(value: &impl Serialize) -> Result<String, ron::Error> {
    let pretty = ron::ser::PrettyConfig::new()
        .struct_names(false)
        .compact_arrays(true);
    ron::ser::to_string_pretty(value, pretty)
}
