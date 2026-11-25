//! The steps of a hire, from its rows to an agent that is live.

use anyhow::Result;
use hamlet_messages::address::{Agent, Profile};
use hamlet_messages::agent::{AgentInput, Fill};
use hamlet_messages::profile::Branch;
use hamlet_messages::realm::Done;
use hamlet_messages::workspace::AgentNumber;

use super::change::{Change, Rows};
use super::peer::{self, When};
use super::steps::{self, Cx, Kind, StepId, Steps};
use super::{documents, place, profiles, tasks, tree};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    /// The profile's actor is made from its character, or started.
    Profile,
    /// The profile has the branch `hires/<n>`, at the hire's version
    /// when it has one.
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
    let agent = AgentNumber(u32::try_from(n)?);
    let rows = cx.rows()?;
    match step {
        Step::Profile => {
            profiles::start(
                cx,
                tree::profile_of(&rows, agent).await?,
            )
            .await?;
        }
        Step::Branch => {
            let profile = tree::profile_of(&rows, agent).await?;
            let named = Branch {
                name: tree::branch(agent),
                from: tree::version_of(&rows, agent).await?,
            };
            let branched = cx
                .peers()
                .call(&Profile(profile), &named, When::AfterAnswer)
                .await?;
            if let Done::Refused { why } = branched {
                return Err(peer::refusal(why));
            }
        }
        Step::Create => {
            let input = AgentInput {
                kind: tree::kind_of(&rows, agent).await?,
                task: tasks::standing(&rows, agent)
                    .await?
                    .on_the_wire(),
                place: place::of(&rows, agent).await?,
                brief: None,
                copies: documents::for_agent(&rows, agent).await?,
            };
            cx.peers().create(&Agent(agent), &input).await?;
        }
        Step::Fill => {
            let fill = Fill {
                profile: tree::profile_of(&rows, agent).await?,
            };
            cx.peers()
                .call(&Agent(agent), &fill, When::AfterAnswer)
                .await?;
        }
        Step::Reach => {
            cx.want_reach(agent).await?;
        }
        Step::Live => {
            return Ok(vec![tree::Change::Live { agent }.into()]);
        }
    }
    Ok(Vec::new())
}

/// Whether `agent`'s hire did `step`, or has no row of steps.
pub(super) async fn reached(
    rows: &Rows<'_>,
    agent: AgentNumber,
    step: Step,
) -> Result<bool> {
    let Some(row) = steps::row(rows, row(agent)).await? else {
        return Ok(true);
    };
    let at = |name: &str| {
        <Step as Steps>::ALL
            .iter()
            .position(|each| each.name() == name)
    };
    Ok(row.done.as_deref().and_then(at) >= at(step.name()))
}
