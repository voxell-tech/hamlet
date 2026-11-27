//! A lead's `end_hire` and `merge_hires` of a hire on a chief's task,
//! refused (H7): the hire's agent is asked before the act runs, since
//! the realm keeps no input's state.

use std::sync::Arc;

use anyhow::Result;
use hamlet_actor::{Action, Ctx, Handler};
use hamlet_messages::Message;
use hamlet_messages::address::Agent;
use hamlet_messages::agent::{AgentKind, OnTask};
use hamlet_messages::realm::hires::Merge;
use hamlet_messages::realm::{
    End, Gate, Named, Refusable, Standing, Tool, ToolCall,
};
use hamlet_messages::sender::sender;
use hamlet_messages::workspace::{AgentNumber, RealmTask};
use serde::de::IgnoredAny;
use serde::{Deserialize, Serialize, Serializer};

use super::act::{self, Act, Decision};
use super::calls::{Calls, Made};
use super::change::{Rows, number};
use super::peer::When;
use super::steps::{self, StepId};
use super::tasks::TaskId;
use super::tree::{self, Status};
use super::{Realm, callers, hire, marker};

/// A society tool whose hires are asked first whether they are on a
/// chief's task.
#[derive(Serialize, Deserialize)]
#[serde(transparent)]
pub(super) struct Asking<M>(pub M);

impl<M: Message> Action for Asking<M> {
    type Output = M::Output;
    const NAME: &'static str = M::NAME;
}

impl<M: Message> Message for Asking<M> {
    type To = M::To;
    type From = M::From;
}

/// An act that ends or merges the hires it names.
pub(super) trait Ends: Act<Agent> {
    /// The hires it names, in the order they are asked.
    fn named(&self) -> Vec<&Named>;
}

impl Ends for End {
    fn named(&self) -> Vec<&Named> {
        vec![&self.hire]
    }
}

impl Ends for Merge {
    fn named(&self) -> Vec<&Named> {
        vec![&self.into, &self.absorbed]
    }
}

/// What an act decides over, told which hires are on a chief's task.
pub(super) trait Told {
    fn on_task(&mut self, on_task: Vec<AgentNumber>);
}

impl<T> Handler<Asking<Tool<T>>> for Realm
where
    T: ToolCall + Ends<Output = T::Answer> + Serialize,
    T::Loaded: Told,
    <T::Answer as Refusable>::Why: From<Standing> + From<Gate>,
{
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        Asking(tool): Asking<Tool<T>>,
    ) -> Result<T::Answer> {
        let name = <Tool<T> as Action>::NAME;
        let by = sender(&ctx, &tool)?;
        let (key, called, act) = act::called(&by, name, tool)?;
        let rows = Rows::outside(&ctx)?;
        let on_task = if marker::find::<IgnoredAny>(&rows, &key)
            .await?
            .is_some()
        {
            Vec::new()
        } else {
            asked(&self, &rows, &by, act.named()).await?
        };
        let ran = act::run_called(
            &self,
            &ctx,
            by,
            (key, called),
            name,
            T::TOOL,
            Asked { act, on_task },
        )
        .await?;
        Ok(callers::to_tool(ran))
    }
}

/// The hires of `named` that their agents say are on a chief's task.
async fn asked(
    realm: &Realm,
    rows: &Rows<'_>,
    by: &Agent,
    named: Vec<&Named>,
) -> Result<Vec<AgentNumber>> {
    let mut on_task = Vec::new();
    for named in named {
        let Some(lineage) = tree::named(rows, by, named).await?
        else {
            continue;
        };
        let hire = lineage.agent.agent;
        if lineage.agent.kind != AgentKind::Hire
            || matches!(lineage.agent.status, Status::Ended(_))
            || !hire::reached(rows, hire, hire::Step::Create).await?
        {
            continue;
        }
        let tasks = chiefs_tasks(rows, hire).await?;
        if tasks.is_empty() {
            continue;
        }
        let minutes = lineage
            .hamlet
            .map_or(0, |hamlet| hamlet.limits.protected_minutes);
        let asking = OnTask {
            tasks,
            within_ms: u64::from(minutes) * 60_000,
        };
        if realm
            .peers
            .call(&Agent(hire), &asking, When::BeforeAnswer)
            .await?
        {
            on_task.push(hire);
        }
    }
    Ok(on_task)
}

/// The chief's tasks `hire` was given: its own, and those of the
/// letters it was sent.
async fn chiefs_tasks(
    rows: &Rows<'_>,
    hire: AgentNumber,
) -> Result<Vec<RealmTask>> {
    let tasks = rows
        .values::<TaskId>(
            "SELECT id FROM task WHERE agent = ?1 AND origin = \
             'chief' UNION SELECT task.id FROM letter JOIN task ON \
             task.id = letter.task WHERE letter.receiver = ?1 AND \
             task.origin = 'chief' ORDER BY 1",
            vec![number(hire)],
        )
        .await?;
    Ok(tasks.into_iter().map(TaskId::on_the_wire).collect())
}

/// An act with the hires found on a chief's task when it was called.
/// It is kept for a chief as the act alone.
struct Asked<A> {
    act: A,
    on_task: Vec<AgentNumber>,
}

impl<A: Serialize> Serialize for Asked<A> {
    fn serialize<S: Serializer>(
        &self,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        self.act.serialize(serializer)
    }
}

impl<A> Act<Agent> for Asked<A>
where
    A: Act<Agent>,
    A::Loaded: Told,
{
    type Output = A::Output;
    type Loaded = A::Loaded;
    type Done = A::Done;

    async fn load(
        &self,
        by: &Agent,
        rows: &Rows<'_>,
    ) -> Result<A::Loaded> {
        let mut loaded = self.act.load(by, rows).await?;
        loaded.on_task(self.on_task.clone());
        Ok(loaded)
    }

    fn decide(
        &self,
        by: &Agent,
        loaded: A::Loaded,
    ) -> Decision<A::Done, A::Output> {
        self.act.decide(by, loaded)
    }

    fn deliveries(&self, by: &Agent, done: &A::Done) -> Calls {
        self.act.deliveries(by, done)
    }

    fn answers_after(&self, done: &A::Done) -> Vec<StepId> {
        self.act.answers_after(done)
    }

    fn answer(&self, done: A::Done, made: &Made) -> A::Output {
        self.act.answer(done, made)
    }

    fn failed(
        &self,
        done: A::Done,
        made: &Made,
        failed: &steps::Failed,
    ) -> A::Output {
        self.act.failed(done, made, failed)
    }
}
