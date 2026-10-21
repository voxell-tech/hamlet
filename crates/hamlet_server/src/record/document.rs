//! The hamlet's members, roads, departures and decisions.

use serde::{Deserialize, Serialize};

use super::By;

/// An agent's stable number within its hamlet (C9).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRef {
    pub hamlet: String,
    pub number: u64,
}

/// A living or archived agent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Agent {
    pub number: u64,
    pub name: String,
    pub purpose: String,
    pub since: i64,
    pub model: Option<String>,
    pub character: String,
}

/// A road end tied to an agent or to the office.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum End {
    Office,
    Agent(u64),
}

/// A road and its recorded traffic.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Road {
    pub a: End,
    pub b: End,
    pub messages: u64,
    pub last_from: Option<End>,
    pub last_used: i64,
    pub since: i64,
    pub by: By,
}

impl Road {
    pub(super) fn joins(&self, a: End, b: End) -> bool {
        (self.a == a && self.b == b) || (self.a == b && self.b == a)
    }

    pub(super) fn touches(&self, number: u64) -> bool {
        self.a == End::Agent(number) || self.b == End::Agent(number)
    }

    pub(super) fn new(a: End, b: End, at: i64, by: By) -> Self {
        Self {
            a,
            b,
            messages: 0,
            last_from: None,
            last_used: 0,
            since: at,
            by,
        }
    }
}

/// A choice kept with its author and reason (H17).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decision {
    pub at: i64,
    pub by: By,
    pub reason: String,
    pub kind: DecisionKind,
}

/// The record change a decision describes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DecisionKind {
    Created {
        agent: u64,
        name: String,
        purpose: String,
    },
    Model {
        agent: u64,
        model: String,
    },
    SkillGiven {
        to: u64,
        skill: String,
        from: Option<String>,
    },
    Removed {
        agent: u64,
        name: String,
        archive: String,
    },
    Restored {
        agent: u64,
        name: String,
        archive: String,
    },
    Erased {
        agent: u64,
        name: String,
        archive: String,
    },
    Merged {
        keep: u64,
        absorb: u64,
        purpose: String,
        notes: MergeNotes,
    },
    RoadBuilt {
        a: End,
        b: End,
    },
    RoadRemoved {
        a: End,
        b: End,
    },
    MayorChanged {
        from: u64,
        to: u64,
    },
    MovedIn {
        agent: u64,
        name: String,
        hamlet: String,
    },
    MovedOut {
        agent: u64,
        name: String,
        hamlet: String,
    },
    /// The mayor refused the road agent `from` asked for to `to`.
    RoadRefused {
        from: u64,
        to: String,
    },
}

/// The notes selected by a merge's house work.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum MergeNotes {
    Joined,
    Rewritten,
    Plain,
}

/// A file brought into the kept house.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BroughtFile {
    pub path: String,
    pub sha256: String,
}

/// The house manifest supplied by merge planning.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeFiles {
    pub notes_before: String,
    pub notes_after: String,
    pub brought: Vec<BroughtFile>,
    pub clashes: Vec<String>,
}

/// Traffic inherited from an absorbed agent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InheritedRoad {
    pub peer: End,
    pub messages: u64,
    pub fresh: bool,
}

/// The reversible record of a merge (H20).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Merge {
    pub kept: Agent,
    pub files: MergeFiles,
    pub inherited: Vec<InheritedRoad>,
    pub applied: bool,
}

/// A departure's retained agent and roads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Archived {
    pub id: String,
    pub agent: Agent,
    pub at: i64,
    pub by: By,
    pub reason: String,
    pub roads: Vec<Road>,
    pub merge: Option<Merge>,
    pub back: Option<String>,
}

/// Road spend in tokens, millionths of a dollar and milliseconds.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize,
)]
pub struct Amount {
    pub tokens: u64,
    pub micros: u64,
    pub ms: u64,
}

/// The budget window supplied by the spend owner, including its
/// non-road spend.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoadBudget {
    pub start: i64,
    pub end: i64,
    pub month: bool,
    pub other: Amount,
    pub cost: Option<u64>,
    pub ms: Option<u64>,
}

/// The limits read by record operations at their time of effect.
#[derive(
    Clone, Debug, Default, PartialEq, Serialize, Deserialize,
)]
pub struct Limits {
    /// The chief's limits.
    pub chief: Box<crate::limits::Limits>,
    /// The road spend's window, which only tests set.
    pub road_budget: Option<RoadBudget>,
}

/// An open vote's decision hold. The votes owner supplies its motion
/// in words.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenVote {
    pub id: u64,
    pub called: i64,
    pub motion: String,
}

/// An owed chief task, supplied by the instructions owner (H7).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Protection {
    pub agent: u64,
    pub chief: String,
    pub text: String,
    pub done: Option<i64>,
}

/// A hamlet's record and the current constraints on its decisions.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hamlet {
    pub id: String,
    pub chiefs: Vec<String>,
    pub founded: i64,
    pub agents: Vec<Agent>,
    pub mayor: u64,
    pub next: u64,
    pub roads: Vec<Road>,
    pub archive: Vec<Archived>,
    pub decisions: Vec<Decision>,
    pub limits: Limits,
    pub open_votes: Vec<OpenVote>,
    pub protected: Vec<Protection>,
}

/// A committed road charge, retained after the road is removed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoadCharge {
    pub agent: u64,
    pub task: Option<String>,
    pub at: i64,
    pub amount: Amount,
}
