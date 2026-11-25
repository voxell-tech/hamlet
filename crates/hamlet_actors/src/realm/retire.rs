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
use super::tree::{self, Life};
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

/// Retires `agent`, if it was filled and has not come back, and
/// begins the row that drops what it leaves: after the retire, whose
/// save that drop would cut.
pub(super) async fn retire(
    cx: &Cx<'_>,
    agent: AgentNumber,
) -> Result<Vec<change::Change>> {
    let rows = cx.rows()?;
    if tree::life(&rows, agent).await? == Life::Living {
        return Ok(Vec::new());
    }
    if hire::reached(&rows, agent, hire::Step::Fill).await? {
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
