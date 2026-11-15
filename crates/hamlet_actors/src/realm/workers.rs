//! Workers, numbered agents that run one turn for their starter: the
//! `worker` table, `StartWorker` and `WorkerDone`.

pub(super) mod errands;

use std::sync::Arc;

use anyhow::{Context as _, Result};
use hamlet_actor::{BindParam, Ctx};
use hamlet_messages::address::Agent;
use hamlet_messages::agent::{
    AgentInput, AgentKind, Brief, BuiltIn, Input, InputId, Submit,
};
use hamlet_messages::realm::budget::Payer;
use hamlet_messages::realm::workers::{
    Ended, StartWorker, Started, WorkerDone, WorkerRefusal,
};
use hamlet_messages::realm::{
    AgentRefusal, HamletId, HowEnded, Refusable, ToolName,
};
use hamlet_messages::sender::Client;
use hamlet_messages::workspace::AgentNumber;
use serde::{Deserialize, Serialize};

use super::act::{Act, Decided, Decision};
use super::calls::{Calls, Delivery, Made};
use super::change::{self, Affected, Rows, Writer, number};
use super::peer::When;
use super::steps::{self, Cx, Kind, StepId, Steps};
use super::tasks::{self, Origin, TaskId};
use super::{Realm, chiefs, place, tree, versions, waits};

/// The built-in tools a worker may be given, as today's catalogue
/// names them: its file tools and its shells
/// (`hamlet_server/src/work/tools.rs:575-577`).
const BUILT_IN: [&str; 7] = [
    "read",
    "write",
    "edit",
    "search",
    "find",
    "bash",
    "powershell",
];

/// How a worker ended, as its row keeps it.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub(super) enum How {
    Answered,
    Failed,
    /// It ended with its starter.
    Stopped,
}

impl How {
    fn ended(self) -> HowEnded {
        match self {
            Self::Answered => HowEnded::Answered,
            Self::Failed => HowEnded::Failed,
            Self::Stopped => HowEnded::Stopped,
        }
    }
}

#[derive(Debug, PartialEq)]
pub(super) struct Worker {
    agent: AgentNumber,
    /// `None` for a chief's.
    starter: Option<AgentNumber>,
    hamlet: Option<HamletId>,
    brief: String,
    /// Empty for its starter's.
    tools: Vec<ToolName>,
    /// The task its turn is charged to, and its answer comes back
    /// under.
    task: TaskId,
}

#[derive(Debug, PartialEq)]
pub(super) enum Change {
    Start(Worker),
    /// The worker ended, with its answer or why it failed, when it
    /// had not.
    End {
        agent: AgentNumber,
        how: How,
        answer: Option<String>,
    },
}

pub(super) async fn apply(
    writer: &Writer<'_>,
    change: Change,
) -> Result<Affected> {
    match change {
        Change::Start(worker) => start(writer, worker).await?,
        Change::End { agent, how, answer } => {
            end(writer, agent, how, answer).await?;
        }
    }
    Ok(Affected::default())
}

async fn start(writer: &Writer<'_>, worker: Worker) -> Result<()> {
    let tools = worker
        .tools
        .into_iter()
        .map(|tool| tool.name)
        .collect::<Vec<_>>();
    writer
        .execute(
            "INSERT INTO worker (agent, starter, hamlet, brief, \
             tools, task, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            vec![
                number(worker.agent),
                worker.starter.map_or(BindParam::Null, number),
                worker.hamlet.map_or(BindParam::Null, |id| {
                    BindParam::Text(id.0)
                }),
                BindParam::Text(worker.brief),
                BindParam::Text(serde_json::to_string(&tools)?),
                worker.task.column()?,
                BindParam::Integer(writer.at().0),
            ],
        )
        .await?;
    Ok(())
}

async fn end(
    writer: &Writer<'_>,
    agent: AgentNumber,
    how: How,
    answer: Option<String>,
) -> Result<()> {
    writer
        .execute(
            "UPDATE worker SET ended = ?2, answer = ?3 WHERE agent \
             = ?1 AND ended IS NULL",
            vec![
                number(agent),
                BindParam::Text(change::case_name(&how)?),
                answer.map_or(BindParam::Null, BindParam::Text),
            ],
        )
        .await?;
    Ok(())
}

/// The changes that end worker `agent`: its row, its agent and so its
/// reach, and its open waits.
fn ending(
    agent: AgentNumber,
    how: How,
    answer: Option<String>,
) -> Vec<change::Change> {
    vec![
        tree::Change::WorkerEnded {
            agent,
            how: how.ended(),
        }
        .into(),
        Change::End { agent, how, answer }.into(),
        waits::Change::Lapse { agent }.into(),
    ]
}

/// The changes that stop `starter`'s living workers, committed with
/// its end.
pub(super) async fn end_with(
    rows: &Rows<'_>,
    starter: AgentNumber,
) -> Result<Vec<change::Change>> {
    let living = rows
        .values::<u32>(
            "SELECT worker.agent FROM worker JOIN agent ON agent.id \
             = worker.agent WHERE worker.starter = ?1 AND \
             agent.status != 'ended'",
            vec![number(starter)],
        )
        .await?;
    Ok(living
        .into_iter()
        .flat_map(|agent| {
            ending(AgentNumber(agent), How::Stopped, None)
        })
        .collect())
}

struct Row {
    starter: Option<AgentNumber>,
    task: TaskId,
    brief: String,
    tools: Vec<String>,
    ended: Option<How>,
    answer: Option<String>,
}

async fn row_of(rows: &Rows<'_>, agent: AgentNumber) -> Result<Row> {
    let (starter, task, brief, tools, ended, answer) = rows
        .row::<(
            Option<u32>,
            TaskId,
            String,
            String,
            Option<String>,
            Option<String>,
        )>(
            "SELECT starter, task, brief, tools, ended, answer FROM \
             worker WHERE agent = ?1",
            vec![number(agent)],
        )
        .await?
        .with_context(|| format!("agent {} is no worker", agent.0))?;
    Ok(Row {
        starter: starter.map(AgentNumber),
        task,
        brief,
        tools: serde_json::from_str(&tools)?,
        ended: ended.map(change::from_case_name).transpose()?,
        answer,
    })
}

/// Delivers the answer of `worker` to its starter, under the task it
/// was started on. A chief's worker's answer stays in its own
/// conversation.
pub(super) async fn answer(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
    worker: AgentNumber,
) -> Result<()> {
    let row = row_of(&Rows::outside(ctx)?, worker).await?;
    let Some(starter) = row.starter else {
        return Ok(());
    };
    let how = row.ended.with_context(|| {
        format!("worker {} has not ended", worker.0)
    })?;
    let submit = Submit {
        id: InputId(format!("worker/{}", worker.0)),
        task: Some(row.task.on_the_wire()),
        input: Input::WorkerAnswer {
            worker,
            text: words(how, row.answer),
        },
    };
    realm
        .peers
        .call(&Agent(starter), &submit, When::AfterAnswer)
        .await
}

/// What the starter reads of its worker's end, in today's words
/// (`hamlet_server/src/talk/follow.rs:133-139`).
fn words(how: How, answer: Option<String>) -> String {
    match (how, answer) {
        (How::Failed, Some(why)) => {
            format!("It could not answer: {why}")
        }
        (How::Answered, Some(text)) if !text.trim().is_empty() => {
            text
        }
        _ => "It gave no answer.".to_owned(),
    }
}

fn row(agent: AgentNumber) -> StepId {
    StepId {
        kind: Kind::Worker,
        n: u64::from(agent.0),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    /// The workspace holds the worker's reach.
    Reach,
    /// `agent/<n>` is made with its brief.
    Create,
}

impl Steps for Step {
    const ALL: &'static [Self] = &[Self::Reach, Self::Create];

    fn name(self) -> &'static str {
        match self {
            Self::Reach => "reach",
            Self::Create => "create",
        }
    }
}

/// Does `step` of worker `n`'s start, and returns the changes
/// committed with its progress. A worker that ended before its step
/// is not started.
pub(super) async fn run(
    cx: &Cx<'_>,
    n: u64,
    step: Step,
) -> Result<Vec<change::Change>> {
    let agent = AgentNumber(u32::try_from(n)?);
    let rows = cx.rows()?;
    if tree::left(&rows, agent).await?.is_some() {
        return Ok(Vec::new());
    }
    match step {
        Step::Reach => {
            cx.commit(vec![
                versions::Change::WantReach(agent).into(),
            ])
            .await?;
        }
        Step::Create => {
            let worker = row_of(&rows, agent).await?;
            let input = AgentInput {
                kind: AgentKind::Worker,
                task: worker.task.on_the_wire(),
                place: place::of(&rows, agent).await?,
                brief: Some(Brief {
                    instructions: None,
                    text: worker.brief,
                    tools: worker
                        .tools
                        .into_iter()
                        .map(|name| BuiltIn { name })
                        .collect(),
                    model: None,
                }),
            };
            cx.peers().create(&Agent(agent), &input).await?;
        }
    }
    Ok(Vec::new())
}

/// The brief as the worker is given it, when it and the tools pass:
/// each tool a built-in the starter has not turned off.
fn checked(
    start: &StartWorker,
    off: &[ToolName],
) -> Result<String, WorkerRefusal> {
    let unknown = start.tools.iter().find(|tool| {
        !BUILT_IN.contains(&tool.name.as_str()) || off.contains(tool)
    });
    if let Some(tool) = unknown {
        return Err(WorkerRefusal::NoSuchTool {
            tool: tool.name.clone(),
        });
    }
    let brief = start.brief.trim();
    if brief.is_empty() {
        return Err(WorkerRefusal::EmptyBrief);
    }
    Ok(brief.to_owned())
}

/// The worker's agent, its row and its step.
fn started(worker: Worker) -> Decided<AgentNumber> {
    let agent = worker.agent;
    Decided::new(agent)
        .change(tree::Change::Worker { agent })
        .change(Change::Start(worker))
        .change(steps::Change::Begin { row: row(agent) })
}

/// What an agent's start reads: nothing more when the agent is a
/// worker.
pub(super) enum ByAgent {
    AWorker,
    Starter {
        worker: AgentNumber,
        hamlet: Option<HamletId>,
        off: Vec<ToolName>,
        task: TaskId,
    },
}

impl Act<Agent> for StartWorker {
    type Output = Started<AgentRefusal<WorkerRefusal>>;
    type Loaded = ByAgent;
    type Done = AgentNumber;

    async fn load(
        &self,
        by: &Agent,
        rows: &Rows<'_>,
    ) -> Result<ByAgent> {
        let starter = tree::lineage_of(rows, by.0).await?;
        if starter.agent.kind == AgentKind::Worker {
            return Ok(ByAgent::AWorker);
        }
        let named = self.task.as_ref().and_then(TaskId::of);
        let task = match named {
            Some(task)
                if tasks::payer(rows, task).await?.is_some() =>
            {
                task
            }
            _ => tasks::standing(rows, by.0).await?,
        };
        Ok(ByAgent::Starter {
            worker: tree::fresh(rows).await?,
            hamlet: starter.hamlet.map(|hamlet| hamlet.id),
            off: tree::tools::off_for(rows, by.0).await?,
            task,
        })
    }

    fn decide(
        &self,
        by: &Agent,
        loaded: ByAgent,
    ) -> Decision<AgentNumber, Self::Output> {
        let refuse = |why| {
            Decision::Answer(Started::refused(AgentRefusal::Act(why)))
        };
        let ByAgent::Starter {
            worker,
            hamlet,
            off,
            task,
        } = loaded
        else {
            return refuse(WorkerRefusal::AWorker);
        };
        let brief = match checked(self, &off) {
            Ok(brief) => brief,
            Err(why) => return refuse(why),
        };
        if self.hamlet.is_some() {
            return refuse(WorkerRefusal::HamletFromAnAgent);
        }
        Decision::Change(started(Worker {
            agent: worker,
            starter: Some(by.0),
            hamlet,
            brief,
            tools: self.tools.clone(),
            task,
        }))
    }

    fn answer(
        &self,
        worker: AgentNumber,
        _made: &Made,
    ) -> Self::Output {
        Started::Started { worker }
    }
}

pub(super) struct ByChief {
    worker: AgentNumber,
    /// `Err` for a hamlet named that the realm does not hold.
    hamlet: Result<Option<HamletId>, ()>,
    /// The worker's own task.
    task: TaskId,
}

impl Act<Client> for StartWorker {
    type Output = Started<WorkerRefusal>;
    type Loaded = ByChief;
    type Done = AgentNumber;

    async fn load(
        &self,
        _by: &Client,
        rows: &Rows<'_>,
    ) -> Result<ByChief> {
        let hamlet = match &self.hamlet {
            Some(id) => chiefs::hamlet(rows, id)
                .await?
                .map(|hamlet| Some(hamlet.id))
                .ok_or(()),
            None => Ok(None),
        };
        Ok(ByChief {
            worker: tree::fresh(rows).await?,
            hamlet,
            task: tasks::fresh(rows).await?,
        })
    }

    fn decide(
        &self,
        _by: &Client,
        loaded: ByChief,
    ) -> Decision<AgentNumber, Self::Output> {
        let refuse = |why| Decision::Answer(Started::refused(why));
        let brief = match checked(self, &[]) {
            Ok(brief) => brief,
            Err(why) => return refuse(why),
        };
        let Ok(hamlet) = loaded.hamlet else {
            return refuse(WorkerRefusal::NoSuchHamlet);
        };
        let payer = hamlet
            .clone()
            .map_or(Payer::Realm, |hamlet| Payer::Hamlet { hamlet });
        let agent = loaded.worker;
        Decision::Change(
            started(Worker {
                agent,
                starter: None,
                hamlet,
                brief,
                tools: self.tools.clone(),
                task: loaded.task,
            })
            .change(tasks::Change::Open {
                task: loaded.task,
                origin: Origin::Chief,
                agent,
                payer,
            }),
        )
    }

    fn answer(
        &self,
        worker: AgentNumber,
        _made: &Made,
    ) -> Self::Output {
        Started::Started { worker }
    }
}

impl Act<Agent> for WorkerDone {
    type Output = ();
    /// Whether the worker still lives.
    type Loaded = bool;
    type Done = ();

    async fn load(
        &self,
        by: &Agent,
        rows: &Rows<'_>,
    ) -> Result<bool> {
        row_of(rows, by.0).await?;
        Ok(tree::left(rows, by.0).await?.is_none())
    }

    fn decide(&self, by: &Agent, living: bool) -> Decision<(), ()> {
        if !living {
            return Decision::Answer(());
        }
        let (how, answer) = match &self.ended {
            Ended::Answered { text } => (How::Answered, text),
            Ended::Failed { why } => (How::Failed, why),
        };
        let mut decided = Decided::new(());
        decided.changes = ending(by.0, how, Some(answer.clone()));
        Decision::Change(decided)
    }

    fn deliveries(&self, by: &Agent, (): &()) -> Calls {
        Calls::after_answer(Delivery::WorkerAnswer(by.0))
    }

    fn answer(&self, (): (), _made: &Made) {}
}
