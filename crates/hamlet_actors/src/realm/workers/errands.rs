//! Errands, short-lived agents outside the tree, numbered from
//! `2^31`: the `errand` table and `ErrandDone`.

use anyhow::{Result, bail};
use hamlet_messages::address::Agent;
use hamlet_messages::realm::Standing;
use hamlet_messages::realm::workers::{Ended, ErrandDone};
use hamlet_messages::workspace::{AgentNumber, ErrandNumber};
use serde::{Deserialize, Serialize};

use super::super::act::{Act, Decision, Unbuilt};
use super::super::calls::Made;
use super::super::change::{self, Affected, Rows, Writer};
use super::super::reviews;
use super::super::steps::{Cx, Steps};
use super::super::tasks::TaskId;

/// What an errand is for, which decides what its end does.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(in crate::realm) enum Purpose {
    /// The review of the line held on `task`.
    Review { task: TaskId },
}

/// What an errand is given to run its one turn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::realm) struct ErrandInput {
    pub instructions: Option<String>,
    pub text: String,
    /// The task that pays for it.
    pub task: TaskId,
    /// `provider::model`; absent for the realm's.
    pub model: Option<String>,
    /// The most it may spend, in millionths of a dollar.
    pub cap: u64,
}

#[derive(Debug, PartialEq)]
pub(in crate::realm) enum Change {
    /// The errand's row and its step.
    Start {
        n: ErrandNumber,
        purpose: Purpose,
        input: ErrandInput,
    },
}

pub(in crate::realm) async fn apply(
    _writer: &Writer<'_>,
    change: Change,
) -> Result<Affected> {
    match change {
        Change::Start { .. } => bail!("not built yet: U5"),
    }
}

/// The change that starts errand `n`, committed with the caller's
/// own.
#[expect(dead_code, reason = "U6 calls it")]
pub(in crate::realm) fn start(
    purpose: Purpose,
    input: ErrandInput,
    n: ErrandNumber,
) -> Change {
    Change::Start { n, purpose, input }
}

/// The changes an errand's end makes, by its purpose.
#[expect(dead_code, reason = "U5 builds it")]
pub(in crate::realm) fn finished(
    purpose: Purpose,
    ended: &Ended,
) -> Vec<change::Change> {
    match purpose {
        Purpose::Review { task } => reviews::ended(task, ended),
    }
}

/// The refusal of a caller that is no agent of the tree.
pub(in crate::realm) async fn standing(
    _rows: &Rows<'_>,
    _agent: AgentNumber,
) -> Result<Option<Standing>> {
    Ok(None)
}

/// An errand's task, and the most it may spend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::realm) struct Spender {
    pub task: TaskId,
    pub cap: u64,
}

/// The task and cap of `agent`, when it is an errand.
pub(in crate::realm) async fn spender(
    _rows: &Rows<'_>,
    _agent: AgentNumber,
) -> Result<Option<Spender>> {
    Ok(None)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::realm) enum Step {
    /// `agent/<2^31 + n>` is made with its brief.
    Create,
}

impl Steps for Step {
    const ALL: &'static [Self] = &[Self::Create];

    fn name(self) -> &'static str {
        match self {
            Self::Create => "create",
        }
    }
}

/// Does `step` of errand `n`'s start, and returns the changes
/// committed with its progress.
pub(in crate::realm) async fn run(
    _cx: &Cx<'_>,
    _n: u64,
    _step: Step,
) -> Result<Vec<change::Change>> {
    bail!("not built yet: U5")
}

impl Act<Agent> for ErrandDone {
    type Output = ();
    type Loaded = Unbuilt;
    type Done = Unbuilt;

    async fn load(
        &self,
        _by: &Agent,
        _rows: &Rows<'_>,
    ) -> Result<Unbuilt> {
        bail!("not built yet: U5")
    }

    fn decide(
        &self,
        _by: &Agent,
        loaded: Unbuilt,
    ) -> Decision<Unbuilt, ()> {
        match loaded {}
    }

    fn answer(&self, done: Unbuilt, _made: &Made) {
        match done {}
    }
}
