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
use hamlet_messages::realm::oversight::Permission;
use hamlet_messages::realm::workers::{
    BUILT_IN, Ended, StartWorker, Started, WorkerDone, WorkerRefusal,
};
use hamlet_messages::realm::{
    AgentRefusal, HamletId, HowEnded, Refusable, SocietyTool,
    ToolName,
};
use hamlet_messages::sender::Client;
use hamlet_messages::workspace::AgentNumber;

use super::act::{Act, Decided, Decision};
use super::calls::{Calls, Delivery, Made};
use super::change::{self, Affected, Rows, Writer, number};
use super::peer::When;
use super::steps::{self, Cx, Kind, StepId, Steps};
use super::tasks::{self, Origin, TaskId};
use super::{
    Realm, chiefs, documents, oversight, place, record, tree,
    versions, waits,
};

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
        how: HowEnded,
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
    how: HowEnded,
    answer: Option<String>,
) -> Result<()> {
    writer
        .execute(
            "UPDATE worker SET ended = ?2, answer = ?3 WHERE agent \
             = ?1 AND ended IS NULL",
            vec![
                number(agent),
                BindParam::Text(serde_json::to_string(&how)?),
                answer.map_or(BindParam::Null, BindParam::Text),
            ],
        )
        .await?;
    Ok(())
}

/// The changes that end worker `agent`.
pub(super) fn ending(
    agent: AgentNumber,
    how: HowEnded,
    answer: Option<String>,
) -> Vec<change::Change> {
    vec![
        tree::Change::WorkerEnded {
            agent,
            how: how.clone(),
        }
        .into(),
        Change::End { agent, how, answer }.into(),
        waits::Change::Lapse { agent }.into(),
        oversight::Change::Revoke {
            agent,
            permission: Permission::Network,
        }
        .into(),
    ]
}

/// The query of the living workers of the agents `starters` selects.
pub(super) fn living(starters: &str) -> String {
    format!(
        "SELECT worker.agent FROM worker JOIN agent ON agent.id = \
         worker.agent WHERE agent.status != 'ended' AND \
         worker.starter IN ({starters})"
    )
}

/// `starter`'s living workers.
pub(super) async fn living_of(
    rows: &Rows<'_>,
    starter: AgentNumber,
) -> Result<Vec<AgentNumber>> {
    let living = rows
        .values::<u32>(&living("?1"), vec![number(starter)])
        .await?;
    Ok(living.into_iter().map(AgentNumber).collect())
}

/// The changes that stop `starter`'s living workers, committed with
/// its end.
pub(super) async fn end_with(
    rows: &Rows<'_>,
    starter: AgentNumber,
) -> Result<Vec<change::Change>> {
    Ok(living_of(rows, starter)
        .await?
        .into_iter()
        .flat_map(|agent| ending(agent, HowEnded::Stopped, None))
        .collect())
}

struct Row {
    starter: Option<AgentNumber>,
    task: TaskId,
    brief: String,
    tools: Vec<String>,
    ended: Option<HowEnded>,
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
        ended: ended
            .map(|how| serde_json::from_str(&how))
            .transpose()?,
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

/// What the starter reads of its worker's end.
fn words(how: HowEnded, answer: Option<String>) -> String {
    match (how, answer) {
        (HowEnded::Failed, Some(why)) => {
            format!("It could not answer: {why}")
        }
        (HowEnded::Answered, Some(text))
            if !text.trim().is_empty() =>
        {
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
                copies: documents::for_agent(&rows, agent).await?,
            };
            cx.peers().create(&Agent(agent), &input).await?;
        }
    }
    Ok(Vec::new())
}

/// The brief as the worker is given it, when it and the tools pass:
/// each tool a built-in that `starter` has not turned off.
fn checked(
    start: &StartWorker,
    starter: &str,
    off: &[ToolName],
) -> Result<String, WorkerRefusal> {
    let society = start.tools.iter().any(|tool| {
        serde_json::from_value::<SocietyTool>(
            tool.name.clone().into(),
        )
        .is_ok()
    });
    if society {
        return Err(WorkerRefusal::ASocietyTool);
    }
    let unknown = start.tools.iter().find(|tool| {
        !BUILT_IN.contains(&tool.name.as_str()) || off.contains(tool)
    });
    if let Some(tool) = unknown {
        return Err(WorkerRefusal::NoSuchTool {
            name: starter.to_owned(),
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
        name: String,
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
        let named = match &self.task {
            Some(task) => tasks::named(rows, by.0, task).await?,
            None => None,
        };
        let task = match named {
            Some(task) => task,
            None => tasks::standing(rows, by.0).await?,
        };
        Ok(ByAgent::Starter {
            worker: tree::fresh(rows).await?,
            name: record::name_of(rows, by.0).await?,
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
            name,
            hamlet,
            off,
            task,
        } = loaded
        else {
            return refuse(WorkerRefusal::AWorker);
        };
        let brief = match checked(self, &name, &off) {
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
    hamlet: Result<Option<HamletId>, HamletId>,
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
                .ok_or_else(|| id.clone()),
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
        let brief = match checked(self, "the chief", &[]) {
            Ok(brief) => brief,
            Err(why) => return refuse(why),
        };
        let hamlet = match loaded.hamlet {
            Ok(hamlet) => hamlet,
            Err(id) => {
                return refuse(WorkerRefusal::NoSuchHamlet { id });
            }
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
            Ended::Answered { text } => (HowEnded::Answered, text),
            Ended::Failed { why } => (HowEnded::Failed, why),
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
