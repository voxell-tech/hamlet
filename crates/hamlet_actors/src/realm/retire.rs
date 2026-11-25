//! The `retire` row: an ended hire's agent stops its turn and saves
//! its files to its branch, so a later merge or restore reads them.

use anyhow::Result;
use hamlet_actor_error::ActorError;
use hamlet_messages::address::Agent;
use hamlet_messages::agent::Retire;
use hamlet_messages::workspace::AgentNumber;

use super::calls::Calls;
use super::change::{self, Rows};
use super::peer::When;
use super::steps::{self, Cx, Kind, StepId, Steps};
use super::{drop_parts, hire};

/// The row that retires `agent`.
pub(super) fn row(agent: AgentNumber) -> StepId {
    StepId {
        kind: Kind::Retire,
        n: u64::from(agent.0),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    Retire,
}

impl Steps for Step {
    const ALL: &'static [Self] = &[Self::Retire];

    fn name(self) -> &'static str {
        match self {
            Self::Retire => "retire",
        }
    }
}

pub(super) async fn run(
    cx: &Cx<'_>,
    n: u64,
    step: Step,
) -> Result<Vec<change::Change>> {
    let agent = AgentNumber(u32::try_from(n)?);
    match step {
        Step::Retire => retire(cx, agent).await,
    }
}

/// Retires `agent`, if it was filled, and begins the row that drops
/// what it leaves: after the retire, whose save that drop would cut.
pub(super) async fn retire(
    cx: &Cx<'_>,
    agent: AgentNumber,
) -> Result<Vec<change::Change>> {
    if reached(&cx.rows()?, agent, hire::Step::Fill).await? {
        let to = Agent(agent);
        unless_gone(
            cx.peers().call(&to, &Retire, When::AfterAnswer).await,
        )?;
    }
    Ok(vec![dropping(agent)])
}

fn dropping(agent: AgentNumber) -> change::Change {
    steps::Change::Begin {
        row: drop_parts::row(agent),
    }
    .into()
}

pub(super) async fn failed(
    _rows: &Rows<'_>,
    n: u64,
) -> Result<(Vec<change::Change>, Calls)> {
    let agent = AgentNumber(u32::try_from(n)?);
    let close = steps::Change::Close { row: row(agent) };
    Ok((vec![close.into(), dropping(agent)], Calls::none()))
}

/// Whether `agent`'s hire did `step`, or has no row of steps.
pub(super) async fn reached(
    rows: &Rows<'_>,
    agent: AgentNumber,
    step: hire::Step,
) -> Result<bool> {
    let Some(row) = steps::row(rows, hire::row(agent)).await? else {
        return Ok(true);
    };
    let at = |name: &str| {
        <hire::Step as Steps>::ALL
            .iter()
            .position(|each| each.name() == name)
    };
    Ok(row.done.as_deref().and_then(at) >= at(step.name()))
}

/// `called`'s answer, or none when its actor is gone: an erased
/// hire's agent and an erased profile destroy themselves.
pub(super) fn unless_gone<T>(called: Result<T>) -> Result<Option<T>> {
    match called {
        Ok(answer) => Ok(Some(answer)),
        Err(error) => {
            let answer = ActorError::extract(&error);
            if (answer.group(), answer.code())
                == ("actor", "not_found")
            {
                Ok(None)
            } else {
                Err(error)
            }
        }
    }
}
