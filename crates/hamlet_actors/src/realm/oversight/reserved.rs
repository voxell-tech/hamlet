//! The society tools the limits reserve for a chief: kept in a wait
//! until one allows or denies them.

use std::fmt::Display;

use anyhow::{Context as _, Result};
use hamlet_messages::address::Agent;
use hamlet_messages::agent::AgentKind;
use hamlet_messages::realm::client::Answer;
use hamlet_messages::realm::world::WaitId;
use hamlet_messages::realm::{
    Done, End, Gate, Hire, HireAnswer, LayRoad, Named, SocietyTool,
    TakeUpRoad,
};
use hamlet_messages::workspace::{
    AgentNumber, ConversationId, RealmTask,
};
use hamlet_protocol::client::Choice;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::super::act::{Act, Decided, Decision};
use super::super::change::{self, Rows};
use super::super::chiefs::limits::Reserved;
use super::super::tasks::TaskId;
use super::super::waits::{self, Kind, Raised, Ruling};
use super::super::{record, tree};

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

impl Held {
    /// Today's tool and its arguments, as the chief reads them.
    fn subject(&self) -> String {
        let mut args = Map::new();
        let mut arg = |name: &str, value: Option<String>| {
            if let Some(value) = value {
                args.insert(name.to_owned(), Value::String(value));
            }
        };
        let tool = match self {
            Self::Hire(hire) => {
                arg(
                    "hamlet",
                    hire.hamlet.as_ref().map(|h| h.0.clone()),
                );
                arg("name", hire.name.clone());
                arg("purpose", hire.purpose.clone());
                arg(
                    "scholar",
                    hire.profile.map(|profile| {
                        format!("scholar#{}", profile.0)
                    }),
                );
                arg(
                    "under",
                    hire.under.as_ref().map(Named::to_string),
                );
                "hire"
            }
            Self::End(end) => {
                arg("hire", Some(end.hire.to_string()));
                arg("reason", end.why.clone());
                "end_hire"
            }
            Self::LayRoad(LayRoad { a, b, why })
            | Self::TakeUpRoad(TakeUpRoad { a, b, why }) => {
                arg("a", Some(a.to_string()));
                arg("b", Some(b.to_string()));
                arg("reason", why.clone());
                if matches!(self, Self::LayRoad(_)) {
                    "lay_road"
                } else {
                    "take_up_road"
                }
            }
        };
        format!("{tool} {}", Value::Object(args))
    }
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
        | SocietyTool::ReadRecord
        | SocietyTool::CreateProfile
        | SocietyTool::ReadProfile
        | SocietyTool::EditProfile
        | SocietyTool::Tag
        | SocietyTool::Revert
        | SocietyTool::AskLibrarian
        | SocietyTool::SubmitSkill => None,
    }
}

/// A call the limits reserve for a chief.
pub(in crate::realm) enum Kept {
    /// The wait that keeps it, with the changes that open it.
    Waits(Decided<WaitId>),
    /// A chief's refusals on its task refuse it unasked.
    Refused(Gate),
}

/// What becomes of `act`, called by `by` in a turn on `task` and in
/// `conversation`, when the limits of `by`'s hamlet reserve `tool`
/// for a chief. Only a hire has a hamlet: the Hand's and a worker's
/// acts are never reserved.
pub(in crate::realm) async fn reserve(
    rows: &Rows<'_>,
    tool: SocietyTool,
    by: AgentNumber,
    task: Option<&RealmTask>,
    conversation: Option<&ConversationId>,
    act: &impl Serialize,
) -> Result<Option<Kept>> {
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
    let subject =
        ciborium::from_reader::<Held, _>(detail.as_slice())?
            .subject();
    let task = task.and_then(TaskId::of);
    if let Some(task) = task
        && let Some(why) =
            waits::refused_unasked(rows, by, task, &[], &subject)
                .await?
    {
        return Ok(Some(Kept::Refused(why)));
    }
    let wait = waits::next(rows).await?;
    Ok(Some(Kept::Waits(Decided::new(wait).change(
        waits::Change::Raise {
            wait,
            raised: Raised {
                agent: by,
                kind: Kind::Reserved,
                task,
                conversation: conversation.cloned(),
                subject,
                detail,
            },
        },
    ))))
}

/// What a reserved act's wait adds to the chief's `answer` on wait
/// `wait`: on allow, the act's changes and decisions, decided now as
/// its caller, and the words its caller is told.
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
    let mut ruling =
        match ciborium::from_reader::<Held, _>(detail.as_slice())? {
            Held::Hire(act) => replay(rows, &by, act).await?,
            Held::End(act) => replay(rows, &by, act).await?,
            Held::LayRoad(act) => replay(rows, &by, act).await?,
            Held::TakeUpRoad(act) => replay(rows, &by, act).await?,
        };
    if ruling.told.is_none() {
        ruling.told = ran(rows, &ruling.entries).await?;
    }
    Ok(ruling)
}

/// What `act` decides now as `by`: its changes and decisions, or the
/// words of the refusal it meets.
async fn replay<A>(
    rows: &Rows<'_>,
    by: &Agent,
    act: A,
) -> Result<Ruling>
where
    A: Act<Agent>,
    A::Output: Refused,
{
    let loaded = act.load(by, rows).await?;
    Ok(match act.decide(by, loaded) {
        Decision::Change(decided) => Ruling {
            changes: decided.changes,
            entries: decided.entries,
            ..Ruling::default()
        },
        Decision::Answer(output)
        | Decision::WaitFor {
            refusal: output, ..
        } => Ruling {
            told: output.words(),
            ..Ruling::default()
        },
        Decision::Again(_) => Ruling::default(),
    })
}

/// Today's words for what a reserved act did, from its decision.
async fn ran(
    rows: &Rows<'_>,
    entries: &[record::Entry],
) -> Result<Option<String>> {
    let name = async |agent| record::name_of(rows, agent).await;
    for entry in entries {
        return Ok(Some(match entry {
            record::Entry::Hired { hire, .. } => {
                format!("Hired hire#{}.", hire.0)
            }
            record::Entry::Ended { hire, .. } => {
                format!("Ended {}.", name(*hire).await?)
            }
            record::Entry::Road { a, b, .. } => format!(
                "Laid a road between {} and {}.",
                name(*a).await?,
                name(*b).await?
            ),
            record::Entry::Unroad { a, b, .. } => format!(
                "Took up the road between {} and {}.",
                name(*a).await?,
                name(*b).await?
            ),
            _ => continue,
        }));
    }
    Ok(None)
}

/// An act's answer, read for the words of its refusal.
trait Refused {
    fn words(self) -> Option<String>;
}

impl<W: Display> Refused for Done<W> {
    fn words(self) -> Option<String> {
        match self {
            Self::Done => None,
            Self::Refused { why } => Some(why.to_string()),
        }
    }
}

impl<W: Display> Refused for HireAnswer<W> {
    fn words(self) -> Option<String> {
        match self {
            Self::Hired { .. } => None,
            Self::Refused { why } => Some(why.to_string()),
        }
    }
}
