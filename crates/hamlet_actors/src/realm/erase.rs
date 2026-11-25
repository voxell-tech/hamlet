//! A chief's `/erase` of an ended hire, and the `erase` row that
//! drops its branch and removes its agent.

use anyhow::Result;
use hamlet_messages::address::{Agent, Profile};
use hamlet_messages::agent::{AgentKind, Erase};
use hamlet_messages::profile::DropBranch;
use hamlet_messages::realm::hires::{EndedRefusal, EraseHire};
use hamlet_messages::realm::{AgentName, Done, Named};
use hamlet_messages::sender::Client;
use hamlet_messages::workspace::AgentNumber;

use super::act::{Act, Decided, Decision};
use super::callers::Decider as _;
use super::calls::{Calls, Made};
use super::change::{self, Rows};
use super::peer::{self, When};
use super::steps::{self, Cx, Kind, StepId, Steps};
use super::tree::{self, Lineage, Status};
use super::{hire, merges, record, retire};

/// The row that erases `agent`.
pub(super) fn row(agent: AgentNumber) -> StepId {
    StepId {
        kind: Kind::Erase,
        n: u64::from(agent.0),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    Branch,
    Agent,
}

impl Steps for Step {
    const ALL: &'static [Self] = &[Self::Branch, Self::Agent];

    fn name(self) -> &'static str {
        match self {
            Self::Branch => "branch",
            Self::Agent => "agent",
        }
    }
}

impl Act<Client> for EraseHire {
    type Output = Done<EndedRefusal>;
    type Loaded = Option<Lineage>;
    type Done = AgentName;

    async fn load(
        &self,
        _by: &Client,
        rows: &Rows<'_>,
    ) -> Result<Option<Lineage>> {
        tree::kept_named(rows, &self.hire).await
    }

    fn decide(
        &self,
        by: &Client,
        named: Option<Lineage>,
    ) -> Decision<AgentName, Self::Output> {
        let hire = match ended_hire(&self.hire, named) {
            Ok(hire) => hire,
            Err(why) => {
                return Decision::Answer(Done::Refused { why });
            }
        };
        let agent = hire.agent.agent;
        Decision::Change(
            Decided::new(hire.agent.name)
                .change(tree::Change::Erased { agent })
                .record(record::Entry::Erased {
                    hire: agent,
                    by: by.by(),
                }),
        )
    }

    fn answers_after(&self, hire: &AgentName) -> Vec<StepId> {
        vec![row(hire.agent)]
    }

    fn answer(&self, _: AgentName, _made: &Made) -> Self::Output {
        Done::Done
    }

    fn failed(
        &self,
        hire: AgentName,
        _made: &Made,
        _failed: &steps::Failed,
    ) -> Self::Output {
        Done::Refused {
            why: EndedRefusal::NotErased { hire },
        }
    }
}

/// The ended hire that `named`, found as `found`, names, or why an
/// act on an ended hire refuses it.
pub(super) fn ended_hire(
    named: &Named,
    found: Option<Lineage>,
) -> Result<Lineage, EndedRefusal> {
    let Some(found) = found else {
        return Err(EndedRefusal::NoSuchAgent {
            address: named.to_string(),
        });
    };
    if found.agent.kind != AgentKind::Hire {
        return Err(EndedRefusal::NotAHire {
            name: found.agent.name.name,
        });
    }
    if !matches!(found.agent.status, Status::Ended(_)) {
        return Err(EndedRefusal::NotLeft {
            hire: found.agent.name,
        });
    }
    Ok(found)
}

pub(super) async fn run(
    cx: &Cx<'_>,
    n: u64,
    step: Step,
) -> Result<Vec<change::Change>> {
    let agent = AgentNumber(u32::try_from(n)?);
    let rows = cx.rows()?;
    match step {
        // A hire ended before its Profile step has no branch, and its
        // profile may have no actor. Its retire, and a merge or split
        // of it, read or write the branch first.
        Step::Branch => {
            let mut first = merges::rows_of(&rows, agent).await?;
            first.push(retire::row(agent));
            steps::after(cx, first).await?;
            if merges::kept_its_inbox(&rows, agent).await? {
                return Err(peer::refusal(
                    "the hire holds an inbox its merge did not move",
                ));
            }
            if hire::reached(&rows, agent, hire::Step::Profile)
                .await?
            {
                let dropped = DropBranch {
                    name: tree::branch(agent),
                };
                let profile = tree::profile_of(&rows, agent).await?;
                cx.peers()
                    .call(
                        &Profile(profile),
                        &dropped,
                        When::AfterAnswer,
                    )
                    .await?;
            }
        }
        // Past the branch's drop an erase is not taken back: it runs
        // until done.
        Step::Agent => {
            if hire::reached(&rows, agent, hire::Step::Create).await?
            {
                let to = Agent(agent);
                let erased =
                    cx.peers().call(&to, &Erase, When::AfterAnswer);
                steps::in_passing(retire::unless_gone(erased.await))?;
            }
        }
    }
    Ok(Vec::new())
}

/// Takes back the erase of the hire `n`, which failed for good before
/// its branch was dropped: the hire is ended, not erased.
pub(super) async fn failed(
    rows: &Rows<'_>,
    n: u64,
) -> Result<(Vec<change::Change>, Calls)> {
    let agent = AgentNumber(u32::try_from(n)?);
    let changes = vec![
        steps::Change::Close { row: row(agent) }.into(),
        tree::Change::Unerased { agent }.into(),
        record::taken_back(rows, "erased", Some(agent), row(agent))
            .await?,
    ];
    Ok((changes, Calls::none()))
}
