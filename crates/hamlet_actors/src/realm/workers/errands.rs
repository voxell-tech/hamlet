//! Errands, short-lived agents outside the tree, numbered from
//! `2^31`: the `errand` table and `ErrandDone`.

use anyhow::{Context as _, Result, bail};
use hamlet_actor::BindParam;
use hamlet_messages::address::Agent;
use hamlet_messages::agent::{
    AgentInput, AgentKind, Brief, Place, PlaceVersion,
};
use hamlet_messages::realm::budget::{Budget, SpendRefusal};
use hamlet_messages::realm::workers::{Ended, ErrandDone};
use hamlet_messages::realm::{HowEnded, Standing};
use hamlet_messages::workspace::{
    AgentNumber, ErrandNumber, Numbered,
};

use super::super::act::{Act, Decided, Decision};
use super::super::calls::Made;
use super::super::change::{
    self, Affected, Rows, Writer, integer, number,
};
use super::super::steps::{self, Cx, Kind, StepId, Steps};
use super::super::tasks::TaskId;
use super::super::{documents, reviews};

const ANSWERED: &str = "answered";

const FAILED: &str = "failed";

/// Today's words for an errand that ended with no answer.
const NO_ANSWER: &str = "the errand gave no answer";

/// What an errand is for, which decides what its end does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::realm) enum Purpose {
    /// The review of the line held on `task`.
    Review { task: TaskId },
}

impl Purpose {
    /// The `purpose` and `subject` columns.
    fn columns(&self) -> (&'static str, Option<TaskId>) {
        match self {
            Self::Review { task } => ("review", Some(*task)),
        }
    }

    fn of(purpose: &str, subject: Option<TaskId>) -> Result<Self> {
        match (purpose, subject) {
            ("review", Some(task)) => Ok(Self::Review { task }),
            _ => bail!("an errand for `{purpose}` on {subject:?}"),
        }
    }
}

/// What an errand is given to run its one turn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::realm) struct ErrandInput {
    pub instructions: Option<String>,
    pub text: String,
    /// The task that pays for it.
    pub task: TaskId,
    /// `provider::model`; absent for the realm's.
    pub model: Option<String>,
    /// The most it may spend, in millionths of a dollar.
    pub cap: u64,
}

#[derive(Debug, PartialEq)]
pub(in crate::realm) enum Change {
    /// The errand's row and its step.
    Start {
        n: ErrandNumber,
        purpose: Purpose,
        input: ErrandInput,
    },
    /// Errand `n` ended, when it had not.
    End { n: ErrandNumber, ended: Ended },
}

pub(in crate::realm) async fn apply(
    writer: &Writer<'_>,
    change: Change,
) -> Result<Affected> {
    match change {
        Change::Start { n, purpose, input } => {
            begin(writer, n, purpose, input).await
        }
        Change::End { n, ended } => end(writer, n, ended).await,
    }
}

async fn begin(
    writer: &Writer<'_>,
    n: ErrandNumber,
    purpose: Purpose,
    input: ErrandInput,
) -> Result<Affected> {
    let ErrandInput {
        instructions,
        text,
        task,
        model,
        cap,
    } = input;
    let (purpose, subject) = purpose.columns();
    let text_or_null = |text: Option<String>| {
        text.map_or(BindParam::Null, BindParam::Text)
    };
    writer
        .execute(
            "INSERT INTO errand (n, purpose, task, cap, subject, \
             instructions, text, model, at) VALUES (?1, ?2, ?3, ?4, \
             ?5, ?6, ?7, ?8, ?9)",
            vec![
                integer(n.get())?,
                BindParam::Text(purpose.to_owned()),
                task.column()?,
                integer(cap)?,
                subject
                    .map(TaskId::column)
                    .transpose()?
                    .unwrap_or(BindParam::Null),
                text_or_null(instructions),
                BindParam::Text(text),
                text_or_null(model),
                BindParam::Integer(writer.at().0),
            ],
        )
        .await?;
    steps::apply(writer, steps::Change::Begin { row: row(n) }).await
}

async fn end(
    writer: &Writer<'_>,
    n: ErrandNumber,
    ended: Ended,
) -> Result<Affected> {
    let (how, answer) = match ended {
        Ended::Answered { text } => (ANSWERED, text),
        Ended::Failed { why } => (FAILED, why),
    };
    writer
        .execute(
            "UPDATE errand SET ended = ?2, answer = ?3 WHERE n = ?1 \
             AND ended IS NULL",
            vec![
                integer(n.get())?,
                BindParam::Text(how.to_owned()),
                BindParam::Text(answer),
            ],
        )
        .await?;
    Ok(Affected::default())
}

fn row(n: ErrandNumber) -> StepId {
    StepId {
        kind: Kind::Errand,
        n: u64::from(n.get()),
    }
}

/// The change that starts errand `n`, committed with the caller's
/// own.
pub(in crate::realm) fn start(
    purpose: Purpose,
    input: ErrandInput,
    n: ErrandNumber,
) -> Change {
    Change::Start { n, purpose, input }
}

/// The number the next errand takes.
pub(in crate::realm) async fn next(
    rows: &Rows<'_>,
) -> Result<ErrandNumber> {
    let highest = rows
        .value::<Option<u32>>("SELECT MAX(n) FROM errand", Vec::new())
        .await?
        .flatten();
    highest
        .map_or(Some(0), |n| n.checked_add(1))
        .and_then(ErrandNumber::new)
        .context("every errand number is taken")
}

/// The changes that end errand `n` as `ended`, with what its end
/// makes by its purpose.
fn ending(
    n: ErrandNumber,
    purpose: Purpose,
    ended: Ended,
) -> Vec<change::Change> {
    let made = match purpose {
        Purpose::Review { task } => reviews::ended(task, &ended),
    };
    [
        Change::End { n, ended }.into(),
        steps::Change::Close { row: row(n) }.into(),
    ]
    .into_iter()
    .chain(made)
    .collect()
}

/// The changes that end errand `n`, whose step failed for good, as
/// an errand that gave no answer. None for an errand that ended.
pub(in crate::realm) async fn failed(
    rows: &Rows<'_>,
    n: u64,
) -> Result<Vec<change::Change>> {
    let n = number_of(n)?;
    let errand = errand_of(rows, n).await?;
    if errand.ended.is_some() {
        return Ok(Vec::new());
    }
    let ended = Ended::Failed {
        why: NO_ANSWER.to_owned(),
    };
    Ok(ending(n, errand.purpose, ended))
}

fn number_of(n: u64) -> Result<ErrandNumber> {
    ErrandNumber::new(u32::try_from(n)?)
        .context("a step past the last errand")
}

/// The refusal of a caller that is no agent of the tree.
pub(in crate::realm) async fn standing(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<Option<Standing>> {
    let member = rows
        .value::<i64>(
            "SELECT 1 FROM agent WHERE id = ?1",
            vec![number(agent)],
        )
        .await?;
    Ok(member.is_none().then_some(Standing::NotAMember))
}

/// What a caller may spend as an errand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::realm) enum Spending {
    /// The caller's number is not an errand's.
    NotAnErrand,
    /// An errand that has not ended: its task, and the most it may
    /// spend.
    Open {
        task: TaskId,
        cap: u64,
    },
    Refused(SpendRefusal),
}

/// What `agent` may spend, when its number is an errand's.
pub(in crate::realm) async fn spending(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<Spending> {
    let Numbered::Errand(n) = agent.numbered() else {
        return Ok(Spending::NotAnErrand);
    };
    let Some(errand) = errand(rows, n).await? else {
        return Ok(Spending::Refused(SpendRefusal::Budget(
            Budget::NoSuchTask,
        )));
    };
    Ok(match errand.ended {
        Some(how) => Spending::Refused(SpendRefusal::Standing(
            Standing::YouLeft { how, why: None },
        )),
        None => Spending::Open {
            task: errand.task,
            cap: errand.cap,
        },
    })
}

struct Errand {
    purpose: Purpose,
    task: TaskId,
    cap: u64,
    ended: Option<HowEnded>,
}

async fn errand(
    rows: &Rows<'_>,
    n: ErrandNumber,
) -> Result<Option<Errand>> {
    let found = rows
        .row::<(String, Option<TaskId>, TaskId, u64, Option<String>)>(
            "SELECT purpose, subject, task, cap, ended FROM errand \
             WHERE n = ?1",
            vec![integer(n.get())?],
        )
        .await?;
    let Some((purpose, subject, task, cap, ended)) = found else {
        return Ok(None);
    };
    let ended = match ended.as_deref() {
        None => None,
        Some(ANSWERED) => Some(HowEnded::Answered),
        Some(FAILED) => Some(HowEnded::Failed),
        Some(other) => bail!("errand {} ended `{other}`", n.get()),
    };
    Ok(Some(Errand {
        purpose: Purpose::of(&purpose, subject)?,
        task,
        cap,
        ended,
    }))
}

async fn errand_of(
    rows: &Rows<'_>,
    n: ErrandNumber,
) -> Result<Errand> {
    errand(rows, n)
        .await?
        .with_context(|| format!("errand {} has no row", n.get()))
}

/// The brief errand `n` is made with.
async fn brief(rows: &Rows<'_>, n: ErrandNumber) -> Result<Brief> {
    let (instructions, text, model) = rows
        .row::<(Option<String>, String, Option<String>)>(
            "SELECT instructions, text, model FROM errand WHERE n = \
             ?1",
            vec![integer(n.get())?],
        )
        .await?
        .with_context(|| format!("errand {} has no row", n.get()))?;
    Ok(Brief {
        instructions,
        text,
        tools: Vec::new(),
        model,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::realm) enum Step {
    /// `agent/<2^31 + n>` is made with its brief.
    Create,
}

impl Steps for Step {
    const ALL: &'static [Self] = &[Self::Create];

    fn name(self) -> &'static str {
        match self {
            Self::Create => "create",
        }
    }
}

/// Does `step` of errand `n`'s start, and returns the changes
/// committed with its progress.
pub(in crate::realm) async fn run(
    cx: &Cx<'_>,
    n: u64,
    step: Step,
) -> Result<Vec<change::Change>> {
    let n = number_of(n)?;
    match step {
        Step::Create => {
            let rows = cx.rows()?;
            let errand = errand_of(&rows, n).await?;
            let input = AgentInput {
                kind: AgentKind::Errand,
                task: errand.task.on_the_wire(),
                place: outside(),
                brief: Some(brief(&rows, n).await?),
                copies: documents::for_agent(&rows, n.agent())
                    .await?,
            };
            cx.peers().create(&Agent(n.agent()), &input).await?;
        }
    }
    Ok(Vec::new())
}

/// An errand's place, which never moves: no lead, no hires and no
/// roads.
fn outside() -> Place {
    Place {
        version: PlaceVersion(1),
        lead: None,
        hires: Vec::new(),
        roads: Vec::new(),
        tools_off: Vec::new(),
        folder: None,
    }
}

impl Act<Agent> for ErrandDone {
    type Output = ();
    /// The errand and its purpose, while it has not ended.
    type Loaded = Option<(ErrandNumber, Purpose)>;
    type Done = ();

    async fn load(
        &self,
        by: &Agent,
        rows: &Rows<'_>,
    ) -> Result<Self::Loaded> {
        let Numbered::Errand(n) = by.0.numbered() else {
            bail!("agent {} is no errand", by.0.0);
        };
        let errand = errand_of(rows, n).await?;
        Ok(errand.ended.is_none().then_some((n, errand.purpose)))
    }

    fn decide(
        &self,
        _by: &Agent,
        loaded: Self::Loaded,
    ) -> Decision<(), ()> {
        let Some((n, purpose)) = loaded else {
            return Decision::Again(());
        };
        let mut decided = Decided::new(());
        decided.changes = ending(n, purpose, self.ended.clone());
        Decision::Change(decided)
    }

    fn answer(&self, (): (), _made: &Made) {}
}

#[cfg(test)]
mod tests {
    use hamlet_messages::workspace::RealmTask;

    use super::super::super::testing::Started;
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn a_start_keeps_what_its_step_and_its_spending_read() {
        let started = Started::new().await;
        let task = TaskId::of(&RealmTask("5".to_owned())).unwrap();
        let first = next(&started.rows()).await.unwrap();
        let input = ErrandInput {
            instructions: Some("Answer PASS.".to_owned()),
            text: "a line".to_owned(),
            task,
            model: Some("local::small".to_owned()),
            cap: 40,
        };
        started
            .commit(start(Purpose::Review { task }, input, first))
            .await
            .unwrap();

        let rows = started.rows();
        assert_eq!(
            errand_of(&rows, first).await.unwrap().purpose,
            Purpose::Review { task }
        );
        assert_eq!(
            brief(&rows, first).await.unwrap(),
            Brief {
                instructions: Some("Answer PASS.".to_owned()),
                text: "a line".to_owned(),
                tools: Vec::new(),
                model: Some("local::small".to_owned()),
            }
        );
        assert_eq!(
            spending(&rows, first.agent()).await.unwrap(),
            Spending::Open { task, cap: 40 }
        );
        assert!(
            steps::row(&rows, row(first)).await.unwrap().is_some()
        );
        assert_eq!(
            (first.get(), next(&rows).await.unwrap().get()),
            (0, 1)
        );
        started.stop().await;
    }
}
