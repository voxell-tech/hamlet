//! Workers and errands, agents that run one turn and end, and the
//! allowance a lead or a chief gives a hire.

use hamlet_actor::Action;
use serde::{Deserialize, Serialize};

use super::{ChiefRequest, Done, HamletId, Named, Refusable};
use crate::address::{Agent, Realm};
use crate::agent::BuiltIn;
use crate::workspace::{AgentNumber, RealmTask};
use crate::{Message, worded};

/// Starts a worker, a numbered agent that runs `brief` once, answers
/// its starter and ends.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartWorker {
    pub brief: String,
    /// The built-in tools the worker may use; empty for the
    /// starter's.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<BuiltIn>,
    /// The task that pays; a chief's worker gets a task of its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<RealmTask>,
    /// A chief's worker's hamlet, whose folder it works in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hamlet: Option<HamletId>,
}

impl ChiefRequest for StartWorker {
    const NAME: &'static str = "chief_start_worker";
    type Answer = Started<WorkerRefusal>;
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    bound(
        serialize = "W: Serialize + std::fmt::Display",
        deserialize = "W: Deserialize<'de>"
    )
)]
pub enum Started<W> {
    Started {
        worker: AgentNumber,
    },
    #[serde(serialize_with = "worded")]
    Refused {
        why: W,
    },
}

impl<W> Refusable for Started<W> {
    type Why = W;
    fn refused(why: W) -> Self {
        Self::Refused { why }
    }
}

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkerRefusal {
    #[error("the brief is empty")]
    EmptyBrief,
    #[error("there is no built-in tool `{tool}`")]
    NoSuchTool { tool: String },
    #[error("a worker starts no worker")]
    AWorker,
    #[error("only a chief names a worker's hamlet")]
    HamletFromAnAgent,
    #[error("there is no such hamlet")]
    NoSuchHamlet,
}

/// The end of a worker's or an errand's one turn.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Ended {
    Answered { text: String },
    Failed { why: String },
}

/// A worker's turn ended; the realm ends the worker.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerDone {
    pub ended: Ended,
}

impl Action for WorkerDone {
    type Output = ();
    const NAME: &'static str = "worker_done";
}

impl Message for WorkerDone {
    type To = Realm;
    type From = Agent;
}

/// An errand's turn ended; the realm acts on its purpose.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrandDone {
    pub ended: Ended,
}

impl Action for ErrandDone {
    type Output = ();
    const NAME: &'static str = "errand_done";
}

impl Message for ErrandDone {
    type To = Realm;
    type From = Agent;
}

/// Sets what a hire and everyone under it may spend in a period.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetAllowance {
    pub hire: Named,
    /// In millionths of a dollar. Absent takes the allowance away.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub micros: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

impl ChiefRequest for SetAllowance {
    const NAME: &'static str = "chief_set_allowance";
    type Answer = Done<AllowanceRefusal>;
}

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AllowanceRefusal {
    #[error("no agent is at that address")]
    NoSuchAgent,
    #[error("only a hire has an allowance")]
    NotAHire,
    #[error(
        "an allowance is at most its lead's, {lead_micros} \
         millionths of a dollar"
    )]
    OverLeads { lead_micros: u64 },
}
