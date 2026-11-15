//! Errands, short-lived agents outside the tree, numbered from
//! `2^31`: the `errand` table and `ErrandDone`.

use anyhow::{Context as _, Result, bail};
use hamlet_actor::BindParam;
use hamlet_messages::address::Agent;
use hamlet_messages::agent::{
    AgentInput, AgentKind, Brief, Place, PlaceVersion,
};
use hamlet_messages::realm::Standing;
use hamlet_messages::realm::workers::{Ended, ErrandDone};
use hamlet_messages::workspace::{
    AgentNumber, ErrandNumber, Numbered,
};
use serde::{Deserialize, Serialize};

use super::super::act::{Act, Decided, Decision};
use super::super::calls::Made;
use super::super::change::{
    self, Affected, Rows, Writer, integer, number,
};
use super::super::reviews;
use super::super::steps::{self, Cx, Kind, StepId, Steps};
use super::super::tasks::TaskId;

/// What an errand is for, which decides what its end does.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(in crate::realm) enum Purpose {
    /// The review of the line held on `task`.
    Review { task: TaskId },
}

impl Purpose {
    /// The task the errand is about.
    fn subject(&self) -> TaskId {
        match self {
            Self::Review { task } => *task,
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

/// The `purpose` column: the purpose, and the brief the errand is
/// made with.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Kept {
    purpose: Purpose,
    brief: Brief,
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
    let subject = purpose.subject();
    let kept = Kept {
        purpose,
        brief: Brief {
            instructions,
            text,
            tools: Vec::new(),
            model,
        },
    };
    writer
        .execute(
            "INSERT INTO errand (n, purpose, task, cap, subject, \
             at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            vec![
                integer(n.get())?,
                BindParam::Text(serde_json::to_string(&kept)?),
                task.column()?,
                integer(cap)?,
                subject.column()?,
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
        Ended::Answered { text } => ("answered", text),
        Ended::Failed { why } => ("failed", why),
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
#[cfg_attr(not(test), expect(dead_code, reason = "U6 calls it"))]
pub(in crate::realm) fn start(
    purpose: Purpose,
    input: ErrandInput,
    n: ErrandNumber,
) -> Change {
    Change::Start { n, purpose, input }
}

/// The number the next errand takes.
#[cfg_attr(not(test), expect(dead_code, reason = "U6 calls it"))]
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

/// The changes an errand's end makes, by its purpose.
pub(in crate::realm) fn finished(
    purpose: Purpose,
    ended: &Ended,
) -> Vec<change::Change> {
    match purpose {
        Purpose::Review { task } => reviews::ended(task, ended),
    }
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

/// An errand's task, and the most it may spend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::realm) struct Spender {
    pub task: TaskId,
    pub cap: u64,
}

/// The task and cap of `agent`, when it is an errand.
pub(in crate::realm) async fn spender(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<Option<Spender>> {
    let Numbered::Errand(n) = agent.numbered() else {
        return Ok(None);
    };
    Ok(errand(rows, n).await?.map(|errand| Spender {
        task: errand.task,
        cap: errand.cap,
    }))
}

struct Errand {
    kept: Kept,
    task: TaskId,
    cap: u64,
    has_ended: bool,
}

async fn errand(
    rows: &Rows<'_>,
    n: ErrandNumber,
) -> Result<Option<Errand>> {
    let found = rows
        .row::<(String, TaskId, u64, Option<String>)>(
            "SELECT purpose, task, cap, ended FROM errand WHERE n = \
             ?1",
            vec![integer(n.get())?],
        )
        .await?;
    let Some((kept, task, cap, ended)) = found else {
        return Ok(None);
    };
    Ok(Some(Errand {
        kept: serde_json::from_str(&kept)?,
        task,
        cap,
        has_ended: ended.is_some(),
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
    let n = ErrandNumber::new(u32::try_from(n)?)
        .context("a step past the last errand")?;
    match step {
        Step::Create => {
            let errand = errand_of(&cx.rows()?, n).await?;
            let input = AgentInput {
                kind: AgentKind::Errand,
                task: errand.task.on_the_wire(),
                place: outside(),
                brief: Some(errand.kept.brief),
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
        Ok((!errand.has_ended).then_some((n, errand.kept.purpose)))
    }

    fn decide(
        &self,
        _by: &Agent,
        loaded: Self::Loaded,
    ) -> Decision<(), ()> {
        let Some((n, purpose)) = loaded else {
            return Decision::Again(());
        };
        let mut decided = Decided::new(()).change(Change::End {
            n,
            ended: self.ended.clone(),
        });
        decided.changes.extend(finished(purpose, &self.ended));
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
        let kept = errand_of(&rows, first).await.unwrap().kept;
        assert_eq!(
            kept,
            Kept {
                purpose: Purpose::Review { task },
                brief: Brief {
                    instructions: Some("Answer PASS.".to_owned()),
                    text: "a line".to_owned(),
                    tools: Vec::new(),
                    model: Some("local::small".to_owned()),
                },
            }
        );
        assert_eq!(
            spender(&rows, first.agent()).await.unwrap(),
            Some(Spender { task, cap: 40 })
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
