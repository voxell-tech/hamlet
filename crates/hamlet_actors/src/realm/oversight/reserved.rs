//! The society tools the limits reserve for a chief: kept in a wait
//! until one allows or denies them.

use anyhow::{Context as _, Result};
use hamlet_messages::address::Agent;
use hamlet_messages::agent::AgentKind;
use hamlet_messages::realm::client::Answer;
use hamlet_messages::realm::world::WaitId;
use hamlet_messages::realm::{
    End, Hire, LayRoad, SocietyTool, TakeUpRoad,
};
use hamlet_messages::workspace::AgentNumber;
use hamlet_protocol::client::Choice;
use serde::{Deserialize, Serialize};

use super::super::act::{Act, Decided, Decision};
use super::super::change::{self, Rows};
use super::super::chiefs::limits::Reserved;
use super::super::tree;
use super::super::waits::{self, Kind, Raised, Ruling};

/// A reservable act, as its wait's `detail` keeps it: written by
/// [`Keeping`], whose `tool` names the case.
#[derive(Deserialize)]
#[serde(tag = "tool", content = "act", rename_all = "snake_case")]
enum Held {
    Hire(Hire),
    End(End),
    LayRoad(LayRoad),
    TakeUpRoad(TakeUpRoad),
}

#[derive(Serialize)]
struct Keeping<'a, A> {
    tool: SocietyTool,
    act: &'a A,
}

/// The limit that reserves `tool`, if one can.
fn limit(tool: SocietyTool) -> Option<fn(&Reserved) -> bool> {
    match tool {
        SocietyTool::Hire => Some(|reserved| reserved.hire),
        SocietyTool::End => Some(|reserved| reserved.end),
        SocietyTool::LayRoad | SocietyTool::TakeUpRoad => {
            Some(|reserved| reserved.roads)
        }
        SocietyTool::Found
        | SocietyTool::Seat
        | SocietyTool::Tell
        | SocietyTool::Ask
        | SocietyTool::RequestRoad
        | SocietyTool::SetTools
        | SocietyTool::AskChief
        | SocietyTool::Object
        | SocietyTool::StartWorker
        | SocietyTool::SetAllowance
        | SocietyTool::ReadBudget
        | SocietyTool::AgreeRoad
        | SocietyTool::SetNickname
        | SocietyTool::SetModel
        | SocietyTool::ReadRecord => None,
    }
}

/// The wait that keeps `act`, with the changes that open it, when the
/// limits of `by`'s hamlet reserve `tool` for a chief. Only a hire
/// has a hamlet: the Hand's and a worker's acts are never reserved.
pub(in crate::realm) async fn reserve(
    rows: &Rows<'_>,
    tool: SocietyTool,
    by: AgentNumber,
    act: &impl Serialize,
) -> Result<Option<Decided<WaitId>>> {
    let Some(reserves) = limit(tool) else {
        return Ok(None);
    };
    let Some(lineage) = tree::lineage(rows, by).await? else {
        return Ok(None);
    };
    if lineage.agent.kind != AgentKind::Hire
        || !lineage
            .hamlet
            .is_some_and(|hamlet| reserves(&hamlet.limits.reserved))
    {
        return Ok(None);
    }
    let mut detail = Vec::new();
    ciborium::into_writer(&Keeping { tool, act }, &mut detail)?;
    let wait = waits::next(rows).await?;
    Ok(Some(Decided::new(wait).change(waits::Change::Raise {
        wait,
        raised: Raised {
            agent: by,
            kind: Kind::Reserved,
            task: None,
            conversation: None,
            subject: format!(
                "{} {}",
                tool.name(),
                serde_json::to_string(act)?
            ),
            detail,
        },
    })))
}

/// What a reserved act's wait adds to the chief's `answer` on wait
/// `wait`: on allow, the act's changes, decided now as its caller.
pub(in crate::realm) async fn ruling(
    rows: &Rows<'_>,
    wait: WaitId,
    answer: &Answer,
) -> Result<Ruling> {
    if answer.0.choice != Choice::Allow {
        return Ok(Ruling::default());
    }
    let (agent, detail) = rows
        .row::<(u32, Vec<u8>)>(
            "SELECT agent, detail FROM wait WHERE id = ?1",
            vec![change::integer(wait.0)?],
        )
        .await?
        .with_context(|| format!("wait {} has no row", wait.0))?;
    let by = Agent(AgentNumber(agent));
    let changes =
        match ciborium::from_reader::<Held, _>(detail.as_slice())? {
            Held::Hire(act) => replay(rows, &by, act).await?,
            Held::End(act) => replay(rows, &by, act).await?,
            Held::LayRoad(act) => replay(rows, &by, act).await?,
            Held::TakeUpRoad(act) => replay(rows, &by, act).await?,
        };
    Ok(Ruling {
        changes,
        picked: None,
    })
}

/// The changes of `act` decided now by `by`; none when it would not
/// run.
async fn replay<A: Act<Agent>>(
    rows: &Rows<'_>,
    by: &Agent,
    act: A,
) -> Result<Vec<change::Change>> {
    let loaded = act.load(by, rows).await?;
    Ok(match act.decide(by, loaded) {
        Decision::Change(decided) => decided.changes,
        Decision::Again(_)
        | Decision::Answer(_)
        | Decision::WaitFor { .. } => Vec::new(),
    })
}
