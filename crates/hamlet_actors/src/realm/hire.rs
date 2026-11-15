//! The steps of a hire, from its rows to an agent that is live.

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    /// The profile's actor is made, or was there.
    Profile,
    /// The profile has the branch `hires/<n>`.
    Branch,
    /// `agent/<n>` is made with its place and its task.
    Create,
    /// The agent's filesystem is filled.
    Fill,
    /// The workspace holds the hire's first reach.
    Reach,
    /// The agent is live, and its lead has a road to it.
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

pub(super) fn row(agent: AgentNumber) -> StepId {
    StepId {
        kind: Kind::Hire,
        n: u64::from(agent.0),
    }
}

/// Does `step` of agent `n`'s hire, with no transaction open, and
/// returns the changes committed with its progress.
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
