//! The society tools reserved for a chief, by the limits or always:
//! kept in a wait until one allows or denies them.

use std::fmt::Display;
use std::sync::Arc;

use anyhow::{Context as _, Result, bail};
use hamlet_actor::Ctx;
use hamlet_messages::address::Agent;
use hamlet_messages::agent::AgentKind;
use hamlet_messages::profile::Revision;
use hamlet_messages::realm::client::Answer;
use hamlet_messages::realm::profiles::{
    EditProfile, RevertProfile, TagProfile,
};
use hamlet_messages::realm::world::WaitId;
use hamlet_messages::realm::{
    DoneTo, End, Gate, HireAnswer, LayRoad, Named, RoadDone,
    SocietyTool, TakeUpRoad,
};
use hamlet_messages::workspace::AgentNumber;
use hamlet_protocol::client::Choice;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::super::act::{Act, Called, Decided, Decision};
use super::super::change::{self, Rows};
use super::super::chiefs::limits::Reserved;
use super::super::profiles::{self, Asking};
use super::super::tasks::TaskId;
use super::super::waits::{self, Kind, Raised, Ruling};
use super::super::{Realm, record, tree};

/// A reservable act, as its wait's `detail` keeps it: written by
/// [`Keeping`], whose `tool` names the case.
#[derive(Deserialize)]
#[serde(tag = "tool", content = "act", rename_all = "snake_case")]
enum Held {
    Hire(tree::Pinned),
    End(End),
    LayRoad(LayRoad),
    TakeUpRoad(TakeUpRoad),
    EditProfile(Asking<EditProfile>),
    Tag(Asking<TagProfile>),
    Revert(Asking<RevertProfile>),
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
            Self::Hire(tree::Pinned { hire, .. }) => {
                arg(
                    "hamlet",
                    hire.hamlet.as_ref().map(|h| h.0.clone()),
                );
                arg("name", hire.name.clone());
                arg("purpose", hire.purpose.clone());
                arg("scholar", hire.profile.map(profiles::address));
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
            Self::EditProfile(Asking { act, .. }) => {
                arg("content", Some(act.text.clone()));
                arg("message", act.message.clone());
                arg("path", Some(act.path.clone()));
                arg("scholar", Some(profiles::address(act.profile)));
                "edit_scholar"
            }
            Self::Tag(Asking { act, .. }) => {
                arg(
                    "note",
                    Some(act.note.clone())
                        .filter(|note| !note.is_empty()),
                );
                arg("scholar", Some(profiles::address(act.profile)));
                arg("tag", Some(act.version.0.clone()));
                "tag"
            }
            Self::Revert(Asking { act, .. }) => {
                arg("commit", Some(revision(&act.what)));
                arg("reason", act.reason.clone());
                arg("scholar", Some(profiles::address(act.profile)));
                "revert"
            }
        };
        format!("{tool} {}", Value::Object(args))
    }
}

/// `what` as a model names a commit to `revert`.
fn revision(what: &Revision) -> String {
    match what {
        Revision::Main => "main".to_owned(),
        Revision::Version { version } => version.0.clone(),
        Revision::Commit { commit } => commit.0.to_string(),
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

/// A call reserved for a chief.
pub(in crate::realm) enum Kept {
    /// The wait that keeps it, with the changes that open it.
    Waits(Decided<WaitId>),
    /// A chief's refusals on its task refuse it unasked.
    Refused(Gate),
}

/// `act`, called by `by` in `called`, kept for a chief when the
/// limits of `by`'s hamlet reserve `tool`.
pub(in crate::realm) async fn reserve(
    rows: &Rows<'_>,
    tool: SocietyTool,
    by: AgentNumber,
    called: &Called,
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
    keep(rows, tool, by, called, act).await.map(Some)
}

/// Keeps `act`, called by `by` in `called`, for a chief.
pub(in crate::realm) async fn keep(
    rows: &Rows<'_>,
    tool: SocietyTool,
    by: AgentNumber,
    called: &Called,
    act: &impl Serialize,
) -> Result<Kept> {
    let mut detail = Vec::new();
    ciborium::into_writer(&Keeping { tool, act }, &mut detail)?;
    let subject =
        ciborium::from_reader::<Held, _>(detail.as_slice())?
            .subject();
    let task = called.task.as_ref().and_then(TaskId::of);
    if let Some(task) = task
        && let Some(why) =
            waits::refused_unasked(rows, by, task, &[], &subject)
                .await?
    {
        return Ok(Kept::Refused(why));
    }
    let wait = waits::next(rows).await?;
    Ok(Kept::Waits(Decided::new(wait).change(
        waits::Change::Raise {
            wait,
            raised: Raised {
                agent: by,
                kind: Kind::Reserved,
                task,
                conversation: called.conversation.clone(),
                subject,
                detail,
            },
        },
    )))
}

/// The caller of the act wait `wait` keeps, and the act.
async fn held(
    rows: &Rows<'_>,
    wait: WaitId,
) -> Result<(Agent, Held)> {
    let (agent, detail) = rows
        .row::<(u32, Vec<u8>)>(
            "SELECT agent, detail FROM wait WHERE id = ?1",
            vec![change::integer(wait.0)?],
        )
        .await?
        .with_context(|| format!("wait {} has no row", wait.0))?;
    Ok((
        Agent(AgentNumber(agent)),
        ciborium::from_reader(detail.as_slice())?,
    ))
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
    let (by, held) = held(rows, wait).await?;
    let mut ruling = match held {
        Held::Hire(act) => replay(rows, &by, act).await?,
        Held::End(act) => replay(rows, &by, act).await?,
        Held::LayRoad(act) => replay(rows, &by, act).await?,
        Held::TakeUpRoad(act) => replay(rows, &by, act).await?,
        Held::EditProfile(_) | Held::Tag(_) | Held::Revert(_) => {
            return Ok(Ruling {
                deferred: true,
                ..Ruling::default()
            });
        }
    };
    if ruling.told.is_none() {
        ruling.told = ran(rows, &ruling.entries).await?;
    }
    Ok(ruling)
}

/// Carries out the change to a profile that the allowed wait `wait`
/// keeps, and gives the words its caller is told.
pub(in crate::realm) async fn carry_out(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
    wait: WaitId,
) -> Result<String> {
    let (by, held) = held(&Rows::outside(ctx)?, wait).await?;
    match held {
        Held::EditProfile(asking) => {
            profiles::carry_out(realm, ctx, by, asking).await
        }
        Held::Tag(asking) => {
            profiles::carry_out(realm, ctx, by, asking).await
        }
        Held::Revert(asking) => {
            profiles::carry_out(realm, ctx, by, asking).await
        }
        Held::Hire(_)
        | Held::End(_)
        | Held::LayRoad(_)
        | Held::TakeUpRoad(_) => {
            bail!("wait {} keeps no change to a profile", wait.0)
        }
    }
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

macro_rules! refused {
    ($($answer:ident),+) => {$(
        impl<W: Display> Refused for $answer<W> {
            fn words(self) -> Option<String> {
                match self {
                    Self::Refused { why } => Some(why.to_string()),
                    _ => None,
                }
            }
        }
    )+};
}

refused!(DoneTo, RoadDone, HireAnswer);
