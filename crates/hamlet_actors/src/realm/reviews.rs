//! A lead's review of a chief's line to its hire, before the hire
//! receives it: the `review` table and its deadline (H10).

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result, bail};
use hamlet_actor::{BindParam, Ctx};
use hamlet_host::HostRef;
use hamlet_messages::address::Agent;
use hamlet_messages::agent::{AgentKind, Input, Submit};
use hamlet_messages::realm::Review;
use hamlet_messages::realm::workers::Ended;
use hamlet_messages::realm::world::{ObjectionSource, WaitKind};
use hamlet_messages::workspace::{
    AgentNumber, ErrandNumber, RealmTask,
};

use super::change::{self, Affected, Rows, Writer, integer, number};
use super::oversight::asking;
use super::peer::When;
use super::settings::{self, Settings};
use super::steps::{self, Cx, Kind, StepId, Steps};
use super::tasks::TaskId;
use super::tree::{self, Lineage};
use super::workers::errands::{self, ErrandInput, Purpose};
use super::{Options, REVIEW_DEADLINE, Realm, waits};

/// How often a review's drive looks for the errand's answer.
const LOOK_AGAIN: Duration = Duration::from_millis(100);

/// The most an errand may spend: today's review has no cap of its
/// own, so only its task's budget bounds it.
const CAP: u64 = i64::MAX.unsigned_abs();

/// The provider every agent runs in a faux realm.
const FAUX: &str = "faux";

#[derive(Debug, PartialEq)]
pub(super) enum Change {
    /// `line`, a chief's line on `task` to `hire`, waits for `lead`'s
    /// review in errand `errand`.
    Hold {
        task: TaskId,
        hire: AgentNumber,
        lead: AgentNumber,
        errand: ErrandNumber,
        line: Submit,
    },
    /// The review of `task` ended as `verdict`, when it had not.
    End { task: TaskId, verdict: Verdict },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Verdict {
    /// Passed, with the lead's note.
    Passed(Option<String>),
    Objected(String),
    /// The errand failed, or the deadline came first.
    Failed,
}

impl Verdict {
    fn outcome(&self) -> &'static str {
        match self {
            Self::Passed(_) => "passed",
            Self::Objected(_) => "objected",
            Self::Failed => "failed",
        }
    }
}

pub(super) async fn apply(
    writer: &Writer<'_>,
    change: Change,
) -> Result<Affected> {
    match change {
        Change::Hold {
            task,
            hire,
            lead,
            errand,
            line,
        } => {
            let at = writer.at().0;
            let due_at =
                at + i64::try_from(REVIEW_DEADLINE.as_millis())?;
            writer
                .execute(
                    "INSERT INTO review (task, hire, lead, errand, \
                     line, due_at, at) VALUES (?1, ?2, ?3, ?4, ?5, \
                     ?6, ?7)",
                    vec![
                        task.column()?,
                        number(hire),
                        number(lead),
                        integer(errand.get())?,
                        BindParam::Blob(crate::cbor(&line)?),
                        BindParam::Integer(due_at),
                        BindParam::Integer(at),
                    ],
                )
                .await?;
            Ok(Affected::default())
        }
        Change::End { task, verdict } => {
            end(writer, task, verdict).await
        }
    }
}

/// Closes the review of `task` as `verdict` when it is open: a note
/// goes on the kept line, and an objection waits for a chief.
async fn end(
    writer: &Writer<'_>,
    task: TaskId,
    verdict: Verdict,
) -> Result<Affected> {
    let closed = writer
        .execute(
            "UPDATE review SET outcome = ?2 WHERE task = ?1 AND \
             outcome IS NULL",
            vec![
                task.column()?,
                BindParam::Text(verdict.outcome().to_owned()),
            ],
        )
        .await?;
    if closed == 0 {
        return Ok(Affected::default());
    }
    let mut kept = held(&writer.rows(), task)
        .await?
        .with_context(|| format!("no review of task {task:?}"))?;
    match verdict {
        Verdict::Passed(Some(note)) => {
            let Input::Line { note: noted, .. } =
                &mut kept.line.input
            else {
                bail!("the line of task {task:?} is no line");
            };
            *noted = Some(format!(
                "{} reviewed this line: {note}",
                named(kept.lead)
            ));
            writer
                .execute(
                    "UPDATE review SET line = ?2 WHERE task = ?1",
                    vec![
                        task.column()?,
                        BindParam::Blob(crate::cbor(&kept.line)?),
                    ],
                )
                .await?;
            Ok(Affected::default())
        }
        Verdict::Objected(reason) => {
            let wait = waits::next(&writer.rows()).await?;
            let raised = waits::Raised {
                agent: kept.hire,
                kind: waits::Kind::Objection,
                task: Some(task),
                conversation: None,
                subject: format!(
                    "A chief told `{}`: {}\n{} objects: {reason}",
                    named(kept.hire),
                    text_of(&kept.line)?,
                    named(kept.lead)
                ),
                detail: crate::cbor(&WaitKind::Objection {
                    reason,
                    source: ObjectionSource::Review,
                })?,
            };
            let affected = waits::apply(
                writer,
                waits::Change::Raise { wait, raised },
            )
            .await?;
            asking::apply(
                writer,
                asking::Change::Object {
                    task,
                    agent: kept.lead,
                    wait,
                    source: ObjectionSource::Review,
                },
            )
            .await?;
            Ok(affected)
        }
        Verdict::Passed(None) | Verdict::Failed => {
            Ok(Affected::default())
        }
    }
}

/// A line kept for its review.
struct Held {
    hire: AgentNumber,
    lead: AgentNumber,
    line: Submit,
    /// When it was kept, in ms since the Unix epoch.
    at: i64,
    outcome: Option<String>,
}

impl Held {
    fn objected(&self) -> bool {
        self.outcome.as_deref() == Some("objected")
    }
}

async fn held(rows: &Rows<'_>, task: TaskId) -> Result<Option<Held>> {
    let found = rows
        .row::<(u32, u32, Vec<u8>, i64, Option<String>)>(
            "SELECT hire, lead, line, at, outcome FROM review WHERE \
             task = ?1",
            vec![task.column()?],
        )
        .await?;
    let Some((hire, lead, line, at, outcome)) = found else {
        return Ok(None);
    };
    Ok(Some(Held {
        hire: AgentNumber(hire),
        lead: AgentNumber(lead),
        line: ciborium::from_reader(line.as_slice())?,
        at,
        outcome,
    }))
}

/// A hire's name as a chief reads it: the realm keeps no other.
fn named(hire: AgentNumber) -> String {
    format!("hire#{}", hire.0)
}

fn text_of(line: &Submit) -> Result<&str> {
    match &line.input {
        Input::Line { text, .. } => Ok(text),
        other => bail!("a review keeps a line, not {other:?}"),
    }
}

/// The changes that hold `line`, a chief's line on `task` to the hire
/// of `lineage`, for its lead's review instead of delivering it.
pub(super) async fn intercept(
    rows: &Rows<'_>,
    lineage: &Lineage,
    task: TaskId,
    line: &Submit,
) -> Result<Option<Vec<change::Change>>> {
    let hire = lineage.agent.agent;
    let (AgentKind::Hire, Some(lead), Some(hamlet)) =
        (lineage.agent.kind, lineage.agent.lead, &lineage.hamlet)
    else {
        return Ok(None);
    };
    let settings = settings::read(rows).await?;
    let reviews = match hamlet.limits.asking.review {
        Review::Always => true,
        Review::Never => false,
        Review::Remote => served_far(&settings),
    };
    if !reviews {
        return Ok(None);
    }
    let text = text_of(line)?;
    let n = errands::next(rows).await?;
    let input = ErrandInput {
        instructions: Some(
            instructions(rows, hire, lead, text).await?,
        ),
        text: format!("A chief told `{}`: {text}", named(hire)),
        task,
        model: settings.model,
        cap: CAP,
    };
    Ok(Some(vec![
        Change::Hold {
            task,
            hire,
            lead,
            errand: n,
            line: line.clone(),
        }
        .into(),
        errands::start(Purpose::Review { task }, input, n).into(),
        steps::Change::Begin { row: row(task)? }.into(),
    ]))
}

/// Whether the lead's model is served from another machine, as
/// today decides it; a model with no name runs no review.
fn served_far(settings: &Settings) -> bool {
    let Some((provider, model)) = settings
        .model
        .as_deref()
        .and_then(|model| model.split_once("::"))
    else {
        return false;
    };
    if provider.is_empty() || model.is_empty() || provider == FAUX {
        return false;
    }
    let address = settings
        .endpoints
        .iter()
        .find(|endpoint| endpoint.name == provider)
        .map(|endpoint| endpoint.base_url.as_str());
    !address.is_some_and(is_local)
}

/// Whether the model server at `address` runs on this machine.
fn is_local(address: &str) -> bool {
    let authority = address
        .split_once("://")
        .map_or(address, |(_, rest)| rest)
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();
    let host = match authority.split_once(']') {
        Some((v6, _)) => format!("{v6}]"),
        None => {
            authority.split(':').next().unwrap_or_default().to_owned()
        }
    }
    .to_ascii_lowercase();
    host == "localhost"
        || host == "[::1]"
        || host == "0.0.0.0"
        || host.starts_with("127.")
}

/// The errand's instructions, in today's words.
async fn instructions(
    rows: &Rows<'_>,
    hire: AgentNumber,
    lead: AgentNumber,
    text: &str,
) -> Result<String> {
    // A hire with no purpose takes the line's, in the same change as
    // the review.
    let given = match tree::purpose_from(hire, text) {
        Some(tree::Change::Purpose { text, .. }) => Some(text),
        _ => None,
    };
    let purpose_of = async |agent: AgentNumber| -> Result<String> {
        let kept = rows
            .value::<Option<String>>(
                "SELECT purpose FROM hire WHERE agent = ?1",
                vec![number(agent)],
            )
            .await?
            .flatten();
        let given = (agent == hire).then(|| given.clone()).flatten();
        Ok(kept.or(given).unwrap_or_default())
    };
    let mut under = Vec::new();
    let mut next = vec![lead];
    while let Some(at) = next.pop() {
        for child in rows
            .values::<u32>(
                "SELECT hire.agent FROM hire JOIN agent ON agent.id \
                 = hire.agent WHERE hire.lead = ?1 AND agent.status \
                 != 'ended' ORDER BY hire.agent",
                vec![number(at)],
            )
            .await?
        {
            let child = AgentNumber(child);
            under.push(format!(
                "- {}: {}",
                named(child),
                purpose_of(child).await?
            ));
            next.push(child);
        }
    }
    let name = named(hire);
    Ok(format!(
        "You are `{}`, and you lead `{name}`, hired to {}. A chief \
         has just instructed `{name}` directly. The work has begun \
         already, and the chief always decides. If the instruction \
         is a mistake for the work you lead, answer `OBJECT: \
         <reason>`. Otherwise answer `PASS`. Most instructions are \
         fine.\n\n# The hires you lead\n{}",
        named(lead),
        purpose_of(hire).await?.trim_end_matches('.'),
        under.join("\n")
    ))
}

/// The changes the end of the review of `task`'s line makes.
pub(super) fn ended(
    task: TaskId,
    ended: &Ended,
) -> Vec<change::Change> {
    vec![
        Change::End {
            task,
            verdict: verdict(ended),
        }
        .into(),
    ]
}

/// The errand's answer as today reads it: `OBJECT: <reason>` objects,
/// and any other answer passes, with the words after `PASS:` as its
/// note.
fn verdict(ended: &Ended) -> Verdict {
    let text = match ended {
        Ended::Answered { text } => text,
        Ended::Failed { .. } => return Verdict::Failed,
    };
    let after = |word: &str| {
        text.trim()
            .split_once(':')
            .filter(|(said, _)| {
                said.trim_matches(|c: char| !c.is_alphabetic())
                    .eq_ignore_ascii_case(word)
            })
            .map(|(_, rest)| rest.trim())
            .filter(|rest| !rest.is_empty())
            .map(str::to_owned)
    };
    match after("OBJECT") {
        Some(reason) => Verdict::Objected(reason),
        None => Verdict::Passed(after("PASS")),
    }
}

fn row(task: TaskId) -> Result<StepId> {
    Ok(StepId {
        kind: Kind::Review,
        n: task.on_the_wire().0.parse()?,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    /// The review's deadline passed, or its errand ended first.
    Deadline,
}

impl Steps for Step {
    const ALL: &'static [Self] = &[Self::Deadline];

    fn name(self) -> &'static str {
        match self {
            Self::Deadline => "deadline",
        }
    }
}

/// Does `step` of the review of task `n`'s line, and returns the
/// changes committed with its progress.
pub(super) async fn run(
    cx: &Cx<'_>,
    n: u64,
    step: Step,
) -> Result<Vec<change::Change>> {
    let task = TaskId::of(&RealmTask(n.to_string()))
        .with_context(|| format!("no task {n} to review"))?;
    match step {
        Step::Deadline => {
            let deadline = HostRef::of(cx.ctx.inner())?
                .value::<Options>()
                .map_or(REVIEW_DEADLINE, |options| {
                    options.review_deadline
                });
            let kept = loop {
                let kept =
                    held(&cx.rows()?, task).await?.with_context(
                        || format!("no review of task {n}"),
                    )?;
                if kept.outcome.is_some() {
                    break kept;
                }
                let due =
                    kept.at + i64::try_from(deadline.as_millis())?;
                let left = u64::try_from(due - now()?).unwrap_or(0);
                if left == 0 {
                    let late = Change::End {
                        task,
                        verdict: Verdict::Failed,
                    };
                    cx.commit(vec![late.into()]).await?;
                    continue;
                }
                tokio::time::sleep(
                    Duration::from_millis(left).min(LOOK_AGAIN),
                )
                .await;
            };
            if !kept.objected() {
                give(cx.realm, &kept).await?;
            }
        }
    }
    Ok(Vec::new())
}

/// The time now, in ms since the Unix epoch.
fn now() -> Result<i64> {
    Ok(i64::try_from(
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),
    )?)
}

/// Gives the hire its line, once the chief lets an objected line go
/// on.
#[expect(
    dead_code,
    reason = "`asking::deliver_held` calls it once U3's file takes \
              the seam"
)]
pub(super) async fn deliver(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
    task: TaskId,
) -> Result<()> {
    let Some(kept) = held(&Rows::outside(ctx)?, task).await? else {
        return Ok(());
    };
    if kept.objected() {
        give(realm, &kept).await?;
    }
    Ok(())
}

/// A second line with the same id is the first one, so a drive
/// started again may give it again.
async fn give(realm: &Arc<Realm>, kept: &Held) -> Result<()> {
    realm
        .peers
        .call(&Agent(kept.hire), &kept.line, When::AfterAnswer)
        .await
}
