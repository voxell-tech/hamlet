//! A lead's review of a chief's line to its hire, before the hire
//! receives it: the `review` table and its deadline (H10).

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, Result, bail, ensure};
use hamlet_actor::{BindParam, Ctx};
use hamlet_host::HostRef;
use hamlet_messages::address::Agent;
use hamlet_messages::agent::{AgentKind, Input, Submit};
use hamlet_messages::realm::Review;
use hamlet_messages::realm::workers::Ended;
use hamlet_messages::realm::world::{ObjectionSource, WaitKind};
use hamlet_messages::workspace::{AgentNumber, RealmTask};

use super::calls::Delivery;
use super::change::{self, Affected, Rows, Writer, number};
use super::oversight::asking;
use super::peer::When;
use super::settings::{self, Settings};
use super::steps::{self, Cx, Kind, StepId, Steps};
use super::tasks::TaskId;
use super::tree::{self, Lineage};
use super::workers::errands::{self, ErrandInput, Purpose};
use super::{
    Options, REVIEW_DEADLINE, Realm, documents, record, waits,
};

/// The most an errand may spend: today's review has no cap of its
/// own, so only its task's budget bounds it.
const CAP: u64 = i64::MAX.unsigned_abs();

/// The provider every agent runs in a faux realm.
const FAUX: &str = "faux";

#[derive(Debug, PartialEq)]
pub(super) enum Change {
    /// `line`, a chief's line on `task` to `hire`, waits for `lead`'s
    /// review.
    Hold {
        task: TaskId,
        hire: AgentNumber,
        lead: AgentNumber,
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

const OBJECTED: &str = "objected";

impl Verdict {
    fn outcome(&self) -> &'static str {
        match self {
            Self::Passed(_) => "passed",
            Self::Objected(_) => OBJECTED,
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
            line,
        } => {
            writer
                .execute(
                    "INSERT INTO review (task, hire, lead, line, \
                     at) VALUES (?1, ?2, ?3, ?4, ?5)",
                    vec![
                        task.column()?,
                        number(hire),
                        number(lead),
                        BindParam::Blob(crate::cbor(&line)?),
                        BindParam::Integer(writer.at().0),
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
/// goes on the kept line, and an objection waits for a chief. Any
/// other end lines up the kept line's delivery to its hire, so
/// whichever end comes first gives it, once.
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
    let rows = writer.rows();
    let mut kept = held(&rows, task)
        .await?
        .with_context(|| format!("no review of task {task:?}"))?;
    if tree::left(&rows, kept.hire).await?.is_some() {
        return Ok(Affected::default());
    }
    match verdict {
        Verdict::Passed(Some(note)) => {
            let Input::Line { note: noted, .. } =
                &mut kept.line.input
            else {
                bail!("the line of task {task:?} is no line");
            };
            *noted = Some(format!(
                "{} reviewed this line: {note}",
                record::address(&rows, kept.lead).await?
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
            Ok(gives(task))
        }
        Verdict::Objected(reason) => {
            let wait = waits::next(&rows).await?;
            let raised = waits::Raised {
                agent: kept.hire,
                kind: waits::Kind::Objection,
                task: Some(task),
                conversation: None,
                subject: format!(
                    "A chief told `{}`: {}\n{} objects: {reason}",
                    record::address(&rows, kept.hire).await?,
                    text_of(&kept.line)?,
                    record::address(&rows, kept.lead).await?
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
        Verdict::Passed(None) | Verdict::Failed => Ok(gives(task)),
    }
}

fn gives(task: TaskId) -> Affected {
    Affected {
        deliveries: vec![Delivery::Kept(task)],
        ..Affected::default()
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
        self.outcome.as_deref() == Some(OBJECTED)
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

/// Whether the chief's line of `task` is kept from its hire: its
/// review is open, or objected with no go from a chief.
pub(super) async fn withholds(
    rows: &Rows<'_>,
    task: TaskId,
) -> Result<bool> {
    Ok(rows
        .value::<bool>(
            "SELECT EXISTS (SELECT 1 FROM review LEFT JOIN \
             objection ON objection.task = review.task WHERE \
             review.task = ?1 AND (review.outcome IS NULL OR \
             (review.outcome = ?2 AND objection.ruling IS NOT \
             'go')))",
            vec![
                task.column()?,
                BindParam::Text(OBJECTED.to_owned()),
            ],
        )
        .await?
        .unwrap_or(false))
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
    let model = documents::model(rows, lead)
        .await?
        .map(|chosen| chosen.model);
    let reviews = match hamlet.limits.asking.review {
        Review::Always => true,
        Review::Never => false,
        Review::Remote => {
            served_far(&settings::read(rows).await?, model.as_deref())
        }
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
        text: format!(
            "A chief told `{}`: {text}",
            record::address(rows, hire).await?
        ),
        task,
        model,
        cap: CAP,
    };
    Ok(Some(vec![
        Change::Hold {
            task,
            hire,
            lead,
            line: line.clone(),
        }
        .into(),
        errands::start(Purpose::Review { task }, input, n).into(),
        steps::Change::Begin { row: row(task)? }.into(),
    ]))
}

/// Whether the lead's model is served from another machine, as
/// today decides it; a model with no name runs no review.
fn served_far(settings: &Settings, model: Option<&str>) -> bool {
    let Some((provider, model)) =
        model.and_then(|model| model.split_once("::"))
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
    url::Url::parse(address).is_ok_and(|url| {
        url.host_str().is_some_and(|host| {
            host == "localhost"
                || host == "[::1]"
                || host == "0.0.0.0"
                || host.starts_with("127.")
        })
    })
}

/// The errand's instructions, in today's words.
async fn instructions(
    rows: &Rows<'_>,
    hire: AgentNumber,
    lead: AgentNumber,
    text: &str,
) -> Result<String> {
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
    for hire in tree::under(rows, lead).await? {
        under.push(format!(
            "- {}: {}",
            record::address(rows, hire).await?,
            purpose_of(hire).await?
        ));
    }
    let name = record::address(rows, hire).await?;
    Ok(format!(
        "You are `{}`, and you lead `{name}`, hired to {}. A chief \
         has just instructed `{name}` directly. The work has begun \
         already, and the chief always decides. If the instruction \
         is a mistake for the work you lead, answer `OBJECT: \
         <reason>`. Otherwise answer `PASS`. Most instructions are \
         fine.\n\n# The hires you lead\n{}",
        record::address(rows, lead).await?,
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

/// The row of the review of `task`, which fails the review still
/// open when its deadline passes.
fn row(task: TaskId) -> Result<StepId> {
    Ok(StepId {
        kind: Kind::Review,
        n: task.on_the_wire().0.parse()?,
    })
}

/// A review's one step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
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

/// Does the deadline of the review row `n`, and returns the changes
/// committed with its progress.
pub(super) async fn run(
    cx: &Cx<'_>,
    n: u64,
    Step::Deadline: Step,
) -> Result<Vec<change::Change>> {
    let task = TaskId::of(&RealmTask(n.to_string()))
        .with_context(|| format!("no task {n} to review"))?;
    let rows = cx.rows()?;
    let kept = held(&rows, task)
        .await?
        .with_context(|| format!("no review of task {n}"))?;
    if kept.outcome.is_some() {
        return Ok(Vec::new());
    }
    let deadline = HostRef::of(cx.ctx.inner())?
        .value::<Options>()
        .map_or(REVIEW_DEADLINE, |options| options.review_deadline);
    let due = kept.at + i64::try_from(deadline.as_millis())?;
    let left = u64::try_from(due - rows.at().0).unwrap_or(0);
    let abort = cx.ctx.abort_signal();
    tokio::select! {
        () = tokio::time::sleep(Duration::from_millis(left)) => {}
        () = abort.cancelled() => {
            let open = held(&cx.rows()?, task)
                .await?
                .is_some_and(|kept| kept.outcome.is_none());
            ensure!(!open, "the realm stopped before review {n}'s deadline");
            return Ok(Vec::new());
        }
    }
    Ok(vec![
        Change::End {
            task,
            verdict: Verdict::Failed,
        }
        .into(),
    ])
}

/// Gives the hire its line, once the chief lets an objected line go
/// on.
pub(super) async fn deliver(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
    task: TaskId,
) -> Result<()> {
    let Some(kept) = held(&Rows::outside(ctx)?, task).await? else {
        return Ok(());
    };
    if kept.objected() {
        give_held(realm, &kept).await?;
    }
    Ok(())
}

/// Gives the hire the line kept on `task`, as its review's end left
/// it.
pub(super) async fn give(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
    task: TaskId,
) -> Result<()> {
    let kept = held(&Rows::outside(ctx)?, task)
        .await?
        .with_context(|| format!("no review of task {task:?}"))?;
    give_held(realm, &kept).await
}

async fn give_held(realm: &Arc<Realm>, kept: &Held) -> Result<()> {
    realm
        .peers
        .call(&Agent(kept.hire), &kept.line, When::AfterAnswer)
        .await
}
