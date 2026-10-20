//! The agents and the hamlets, with their state in words, and a
//! hamlet's record.

use serde::{Deserialize, Serialize};

/// Every agent and hamlet, and the hamlets closed down.
#[derive(
    Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize,
)]
pub struct Roster {
    /// By hamlet: the mayor, the living by their numbers, then those
    /// that are gone.
    pub agents: Vec<RosterAgent>,
    /// The first hamlet first, then by title.
    pub hamlets: Vec<RosterHamlet>,
    pub closed: Vec<ClosedHamlet>,
}

/// An agent, living or gone.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct RosterAgent {
    pub name: String,
    /// Its number in its hamlet's record.
    pub number: u64,
    /// The name of its hamlet on disk.
    pub hamlet: String,
    pub mayor: bool,
    pub purpose: String,
    /// What it does, or how it left, in words.
    pub state: String,
    pub busy: bool,
    pub gone: bool,
    /// The agent it was merged into.
    pub merged_into: Option<String>,
    /// Things it has waiting for the chief.
    pub waiting: u32,
    pub model: String,
    /// Its model and spend, in words.
    pub account: String,
    /// Its spend from its hamlet's budget in the period, as
    /// "Spent today: $0.10"; empty with no account yet.
    pub spent: String,
}

/// A hamlet.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct RosterHamlet {
    /// Its name on disk and in commands.
    pub id: String,
    /// The name its chief gave it.
    pub title: String,
    pub mayor: String,
    pub living: u32,
    pub working: u32,
    pub waiting: u32,
    pub gone: u32,
    /// Its agents at rest, at work or waiting, in words.
    pub state: String,
    /// Its spend against its budget, in words.
    pub account: String,
    /// The folder it works on.
    pub folder: Option<String>,
    /// Its agents ask before each edit and command.
    pub manual: bool,
    /// The mode in a sentence.
    pub mode: String,
    /// The folders it worked on before, the latest first.
    pub before: Vec<String>,
}

/// A hamlet closed down, to be restored or deleted.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct ClosedHamlet {
    pub name: String,
    pub title: String,
}

/// A hamlet's budget, votes, comings and goings and limits.
#[derive(
    Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize,
)]
pub struct Record {
    pub budget: String,
    /// The latest first.
    pub votes: Vec<Vote>,
    /// Who came and went.
    pub record: String,
    /// The text of its limits file.
    pub limits: String,
}

/// The limits a hamlet's own file holds for it alone: its bylaws.
#[derive(
    Clone, PartialEq, Debug, Default, Serialize, Deserialize,
)]
pub struct Bylaws {
    /// The budget starts again every month, else every day.
    pub monthly: bool,
    /// What model calls may cost in a period, in dollars; none is no
    /// cap.
    pub budget_cost: Option<f64>,
    /// What one task may cost, in dollars; none is no cap.
    pub task_cost: Option<f64>,
    /// Messages between agents one task may send.
    pub task_messages: u32,
    /// The price of a road, in dollars.
    pub road_cost: f64,
    /// What the mayor may do without asking.
    pub create_agents: bool,
    pub remove_agents: bool,
    pub merge_agents: bool,
    pub build_roads: bool,
    /// Days of the mayor's decisions its agents are told of.
    pub told_days: u64,
    pub most_agents: u64,
    /// Roads the mayor may build.
    pub most_roads: u64,
    /// The models the mayor may choose, besides the hamlet's own.
    pub models: Vec<String>,
    /// The model its agents run when nobody chose one; none is the
    /// home's default.
    pub model: Option<String>,
    /// Why the file cannot be read; the values are then the
    /// defaults.
    pub problem: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Vote {
    pub id: u64,
    pub report: String,
    /// A chief can still overturn it.
    pub open: bool,
}
