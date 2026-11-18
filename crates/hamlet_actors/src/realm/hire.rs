//! The steps of a hire, from its rows to an agent that is live.

use anyhow::Result;
use hamlet_messages::address::{Agent, Profile};
use hamlet_messages::agent::{AgentInput, Fill};
use hamlet_messages::profile::Branch;
use hamlet_messages::workspace::AgentNumber;

use super::change::Change;
use super::peer::When;
use super::steps::{Cx, Kind, StepId, Steps};
use super::{place, tasks, tree, versions};

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
    let agent = AgentNumber(u32::try_from(n)?);
    let rows = cx.rows()?;
    match step {
        Step::Profile => {
            let profile = tree::profile_of(&rows, agent).await?;
            cx.peers().get_or_start(&Profile(profile)).await?;
        }
        Step::Branch => {
            let profile = tree::profile_of(&rows, agent).await?;
            let named = Branch { name: branch(n) };
            cx.peers()
                .call(&Profile(profile), &named, When::AfterAnswer)
                .await?;
        }
        Step::Create => {
            let input = AgentInput {
                kind: tree::kind_of(&rows, agent).await?,
                task: tasks::standing(&rows, agent)
                    .await?
                    .on_the_wire(),
                place: place::of(&rows, agent).await?,
                brief: None,
                copies: Default::default(),
            };
            cx.peers().create(&Agent(agent), &input).await?;
        }
        Step::Fill => {
            let fill = Fill {
                profile: tree::profile_of(&rows, agent).await?,
                branch: branch(n),
            };
            cx.peers()
                .call(&Agent(agent), &fill, When::AfterAnswer)
                .await?;
        }
        Step::Reach => {
            cx.commit(vec![
                versions::Change::WantReach(agent).into(),
            ])
            .await?;
        }
        Step::Live => {
            return Ok(vec![tree::Change::Live { agent }.into()]);
        }
    }
    Ok(Vec::new())
}

/// The profile's branch a hire is filled from.
fn branch(n: u64) -> String {
    format!("hires/{n}")
}
