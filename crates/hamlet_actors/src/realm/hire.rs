//! The steps of a hire, from its rows to an agent that is live: the
//! Hand's, a mayor's and a hire's alike. The design's steps 1 to 3
//! are the `Hire` action and its answer; these are steps 4 to 9, with
//! the profile's actor made first so that its `Branch` has a callee.

#![expect(
    unused_variables,
    dead_code,
    reason = "the scaffold: U3 fills this file"
)]

use anyhow::Result;
use hamlet_messages::workspace::AgentNumber;

use super::change::Change;
use super::not_built;
use super::steps::{Cx, Kind, StepId, Steps};

/// A hire's steps, by the name its row records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    /// The profile's actor is made, or was there. A call never makes
    /// its callee, so the `Branch` below needs this.
    Profile,
    /// Step 5: the profile has the branch `hires/<n>`.
    Branch,
    /// Step 6: `agent/<n>` is made with its place and its task.
    Create,
    /// Step 7: the agent's filesystem is filled.
    Fill,
    /// Step 8: the workspace holds the hire's first reach.
    Reach,
    /// Step 9: the agent is live, and its lead has a road to it.
    Live,
}

impl Steps for Step {
    const ALL: &'static [Self] = &[
        Self::Profile,
        Self::Branch,
        Self::Create,
        Self::Fill,
        Self::Reach,
        Self::Live,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::Profile => "profile",
            Self::Branch => "branch",
            Self::Create => "create",
            Self::Fill => "fill",
            Self::Reach => "reach",
            Self::Live => "live",
        }
    }
}

/// The row of `agent`'s hire.
pub(super) fn row(agent: AgentNumber) -> StepId {
    StepId {
        kind: Kind::Hire,
        n: u64::from(agent.0),
    }
}

/// Does `step` of agent `n`'s hire, with no transaction open, and
/// returns the changes committed with its progress. It runs again
/// after a kill or a failure, so each callee finds its own earlier
/// work: a profile or an agent already made counts as made, the
/// stub profile answers "done" to a branch that exists, a filled
/// agent answers "done", and the workspace keeps a reach only when
/// it is newer.
pub(super) async fn run(
    cx: &Cx<'_>,
    n: u64,
    step: Step,
) -> Result<Vec<Change>> {
    Err(not_built("U3"))
}

/// The profile's branch a hire is filled from.
fn branch(n: u64) -> String {
    todo!()
}
