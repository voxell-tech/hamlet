//! The calls the realm makes after a commit.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, anyhow};
use futures::future::BoxFuture;
use hamlet_actor::{Ctx, KeepAwakeRegion};
use hamlet_messages::address::Agent;
use hamlet_messages::agent::chat::{Stop, Stopped};
use hamlet_messages::agent::{Input, Submit};
use hamlet_messages::realm::LetterId;
use hamlet_messages::realm::world::WaitId;
use hamlet_messages::workspace::AgentNumber;
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

use super::change::{Affected, Rows};
use super::lanes::{self, Queued, Turn};
use super::oversight::asking;
use super::peer::{self, When};
use super::tasks::TaskId;
use super::tree::{self, Status};
use super::{
    Realm, documents, letters, place, reach, reviews, steps, waits,
    workers,
};

/// A call that carries an act's content.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Delivery {
    /// An input carried whole: a chief's line, or a notice with no
    /// row of its own.
    Line { to: AgentNumber, submit: Submit },
    /// The letter or the question `0`, from its row, to its receiver.
    Letter(LetterId),
    /// The answer to the question `0`, from its row, to the asker's
    /// conversation.
    Answer(LetterId),
    /// The chief's answer to the wait `0`, from its row, to the
    /// workspace.
    ChiefAnswer(WaitId),
    /// The notice that closes the wait `0`, from its row, to its
    /// agent.
    Notice(WaitId),
    /// The answer of the worker `0`, from its row, to its starter.
    WorkerAnswer(AgentNumber),
    /// The letters and answers held on the task `0`, to their
    /// receivers.
    Held(TaskId),
    /// The line a review kept on the task `0`, to its hire.
    Kept(TaskId),
    /// The notice that tells a parcel's sender why it was not sent.
    ParcelRefused { to: AgentNumber, submit: Submit },
    /// The stop `number` of the agent `to`, with the lines it
    /// withdrew.
    Stop { to: AgentNumber, number: u64 },
}

impl Delivery {
    /// Whether the realm makes it again after a failure in passing:
    /// the chief was told its line was taken, the sender its parcel
    /// was sent, the chief its wait was closed, or the caller its
    /// call was answered before its notice, so no repeat of theirs
    /// carries it. A wait's notice made again makes the allowed act
    /// again, under the id its first run asked with.
    fn owed(&self) -> bool {
        match self {
            Self::Kept(_)
            | Self::ParcelRefused { .. }
            | Self::Notice(_) => true,
            Self::Line { submit, .. } => {
                matches!(submit.input, Input::Notice { .. })
            }
            _ => false,
        }
    }

    /// The chief's line it carries, with its agent.
    pub fn said(&self) -> Option<(AgentNumber, &Submit)> {
        match self {
            Self::Line { to, submit }
                if matches!(submit.input, Input::Line { .. }) =>
            {
                Some((*to, submit))
            }
            _ => None,
        }
    }

    /// The agent whose lane it keeps through its retries: a chief's
    /// line and a stop keep theirs.
    fn holds_lane(&self) -> Option<AgentNumber> {
        match self {
            Self::Stop { to, .. } => Some(*to),
            _ => self.said().map(|(to, _)| to),
        }
    }
}

#[derive(Debug, Default, PartialEq)]
pub(super) struct Calls {
    pub before_answer: Vec<Delivery>,
    pub after_answer: Vec<Delivery>,
}

impl Calls {
    pub fn none() -> Self {
        Self::default()
    }

    pub fn after_answer(delivery: Delivery) -> Self {
        Self {
            before_answer: Vec::new(),
            after_answer: vec![delivery],
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct Made {
    failed: Vec<anyhow::Error>,
    /// Each stop's count of the turns it stopped: its agent's, or
    /// the realm's own when the agent did not answer in time.
    stopped: HashMap<AgentNumber, u32>,
}

impl Made {
    /// The turns its stops stopped.
    pub fn stopped(&self) -> u32 {
        self.stopped.values().sum()
    }

    /// The first delivery's failure, as its callee answered it, when
    /// one was not taken.
    pub fn taken(self) -> Result<()> {
        self.failed.into_iter().next().map_or(Ok(()), Err)
    }
}

/// State a commit moved, to carry to the actor that keeps a copy.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum Push {
    /// The agent's place, to the agent.
    Place(AgentNumber),
    /// The agent's reach, to the workspace.
    Reach(AgentNumber),
    /// The steps of a row.
    Drive(steps::StepId),
    /// A part of the copies, to the agents that read it.
    Copies(documents::Copied),
}

impl Push {
    /// The pushes of a commit, in the order they are made: the
    /// drives, then the reaches, then the copies, then the places.
    pub fn of(affected: &Affected) -> Vec<Self> {
        let drives = affected.begun.iter().copied().map(Self::Drive);
        let reaches =
            affected.reaches.iter().copied().map(Self::Reach);
        let copies =
            affected.copies.iter().copied().map(Self::Copies);
        let places = affected.places.iter().copied().map(Self::Place);
        drives.chain(reaches).chain(copies).chain(places).collect()
    }
}

/// Makes `deliveries`, each once the deliveries lined up before it
/// to the same agents are done.
pub(super) async fn deliver_all(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
    deliveries: Vec<Queued>,
    when: When,
) -> Made {
    let delivered = futures::future::join_all(
        deliveries
            .into_iter()
            .map(|queued| deliver(realm, ctx, queued, when)),
    )
    .await;
    let mut made = Made::default();
    for delivered in delivered {
        match delivered {
            Ok(Some((agent, turns))) => {
                made.stopped.insert(agent, turns);
            }
            Ok(None) => {}
            Err(error) => made.failed.push(error),
        }
    }
    made
}

/// Makes `delivery`, giving a stop's count of the turns it stopped
/// with its agent.
async fn deliver(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
    Queued { delivery, mut turn }: Queued,
    when: When,
) -> Result<Option<(AgentNumber, u32)>> {
    if let Some(agent) = delivery.holds_lane() {
        return Ok(held(realm, ctx, agent, delivery, turn).await);
    }
    let owed = delivery
        .owed()
        .then(|| (delivery.clone(), ctx.keep_awake_region()));
    turn.come().await;
    let delivered = make(realm, ctx, &delivery, when).await;
    if let (Err(error), Some((owed, awake))) = (&delivered, owed)
        && !peer::refused(error)
    {
        ctx.register_task(again(
            realm.clone(),
            ctx.clone(),
            owed,
            awake,
        ));
    }
    delivered.map(|()| None)
}

/// The first wait before a delivery that keeps its agent's lane is
/// made again, doubled after each failure up to [`LAST_HOLD`].
const FIRST_HOLD: Duration = Duration::from_millis(50);

const LAST_HOLD: Duration = Duration::from_secs(1);

/// Makes `delivery`, which keeps the lane of `agent` through its
/// retries, on a task of the realm's. A stop gives its count once its
/// first try ends within [`When::Stop`]: the agent's, or else the
/// realm's own.
async fn held(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
    agent: AgentNumber,
    delivery: Delivery,
    turn: Turn,
) -> Option<(AgentNumber, u32)> {
    let stop = match delivery {
        Delivery::Stop { number, .. } => Some(number),
        _ => None,
    };
    let (first, tried) = oneshot::channel();
    ctx.register_task(hold(
        realm.clone(),
        ctx.clone(),
        (agent, delivery),
        turn,
        first,
        ctx.keep_awake_region(),
    ));
    let number = stop?;
    let answered =
        tokio::time::timeout(When::Stop.deadline(), tried).await;
    let turns = match answered {
        Ok(Ok(Ok(Some(stopped)))) => stopped.turns,
        _ => realm.lines.counted(agent, number),
    };
    Some((agent, turns))
}

/// Makes `delivery` to `to` once its turn comes, and again after each
/// failure in passing while it keeps the lane, until it is taken, its
/// callee refuses it, `to` has left, a stop withdrew its line or the
/// realm stops, while `_awake` keeps the realm from sleeping. `first`
/// takes its first try's outcome.
async fn hold(
    realm: Arc<Realm>,
    ctx: Ctx<Realm>,
    (to, delivery): (AgentNumber, Delivery),
    mut turn: Turn,
    first: oneshot::Sender<Result<Option<Stopped>>>,
    _awake: KeepAwakeRegion,
) {
    turn.come().await;
    if let Some((to, submit)) = delivery.said()
        && !realm.lines.carry(to, submit)
    {
        return;
    }
    let stopping = ctx.abort_signal();
    let mut first = Some(first);
    let mut wait = FIRST_HOLD;
    loop {
        let woken = realm.lines.stopping();
        let error = match attempt(&realm, &ctx, to, &delivery).await {
            Tried::Over(outcome) => {
                if let Some(first) = first.take() {
                    let _ = first.send(outcome);
                }
                return;
            }
            Tried::Again(error) => error,
        };
        if let Some(first) = first.take() {
            tracing::warn!(?delivery, "a delivery failed: {error:#}");
            let _ = first.send(Err(error));
        }
        tokio::select! {
            () = stopping.cancelled() => return,
            () = woken => {}
            () = tokio::time::sleep(wait) => {}
        }
        wait = (wait * 2).min(LAST_HOLD);
    }
}

enum Tried {
    /// It is done: taken, refused, or no longer to be made.
    Over(Result<Option<Stopped>>),
    /// It failed in passing.
    Again(anyhow::Error),
}

/// One try of `delivery` to `to`, which keeps its lane. A line a
/// stop withdrew is not tried, nor a delivery to an agent still
/// being hired.
async fn attempt(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
    to: AgentNumber,
    delivery: &Delivery,
) -> Tried {
    let line = delivery.said().map(|(_, submit)| &submit.id);
    let status = status(ctx, to).await;
    if let Some(id) = line
        && !realm.lines.attempt(id)
    {
        return Tried::Over(Ok(None));
    }
    let failed = |error| {
        if let Some(id) = line {
            realm.lines.failed(id);
        }
        Tried::Again(error)
    };
    match status {
        Err(error) => return failed(error),
        Ok(None | Some(Status::Ended(_))) => {
            if let Some(id) = line {
                realm.lines.lost(id);
            }
            return Tried::Over(Ok(None));
        }
        Ok(Some(Status::Hiring)) => {
            return failed(anyhow!("agent {} is being hired", to.0));
        }
        Ok(Some(Status::Live)) => {}
    }
    match send(realm, ctx, delivery, When::AfterAnswer).await {
        Ok(stopped) => {
            if let Some(id) = line {
                realm.lines.given(id);
            }
            Tried::Over(Ok(stopped))
        }
        Err(error) if peer::refused(&error) => {
            if let Some(id) = line {
                realm.lines.lost(id);
            }
            Tried::Over(Err(error))
        }
        Err(error) => failed(error),
    }
}

/// The status of the agent `to`, when it has a row.
async fn status(
    ctx: &Ctx<Realm>,
    to: AgentNumber,
) -> Result<Option<Status>> {
    let rows = Rows::outside(ctx)?;
    Ok(tree::lineage(&rows, to)
        .await?
        .map(|lineage| lineage.agent.status))
}

/// Makes `delivery`, which failed in passing, again on the steps'
/// schedule until it is taken, its callee refuses it, its receiver
/// has left or the realm stops, while `_awake` keeps the realm from
/// sleeping. A repeat of an input the agent took into the same
/// conversation is that input.
async fn again(
    realm: Arc<Realm>,
    ctx: Ctx<Realm>,
    delivery: Delivery,
    _awake: KeepAwakeRegion,
) {
    let (realm, ctx) = (&realm, &ctx);
    steps::retried(ctx, Some(steps::FIRST_RETRY), move || {
        let delivery = delivery.clone();
        async move {
            let rows = Rows::outside(ctx)?;
            for agent in lanes::receivers(&rows, &delivery).await? {
                if tree::left(&rows, agent).await?.is_some() {
                    return Ok(());
                }
            }
            make(realm, ctx, &delivery, When::AfterAnswer).await
        }
    })
    .await;
}

/// Makes `delivery`, and tells its failure.
async fn make(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
    delivery: &Delivery,
    when: When,
) -> Result<()> {
    let made = send(realm, ctx, delivery, when).await;
    if let Err(error) = &made {
        tracing::warn!(?delivery, "a delivery failed: {error:#}");
    }
    made.map(drop)
}

/// Makes `delivery`, giving a stop's answer.
async fn send(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
    delivery: &Delivery,
    when: When,
) -> Result<Option<Stopped>> {
    let delivered = match delivery {
        Delivery::Stop { to, number } => {
            let stop = Stop {
                number: *number,
                withdrawn: realm.lines.withdrawn(*to, *number),
            };
            return realm
                .peers
                .call(&Agent(*to), &stop, when)
                .await
                .map(Some);
        }
        Delivery::Line { to, submit }
        | Delivery::ParcelRefused { to, submit } => {
            realm.peers.call(&Agent(*to), submit, when).await
        }
        Delivery::Letter(_) | Delivery::Answer(_) => {
            letters::deliver(realm, ctx, delivery.clone()).await
        }
        Delivery::ChiefAnswer(wait) => {
            waits::deliver(realm, ctx, *wait).await
        }
        Delivery::Notice(wait) => {
            waits::notice(realm, ctx, *wait).await
        }
        Delivery::WorkerAnswer(worker) => {
            workers::answer(realm, ctx, *worker).await
        }
        Delivery::Held(task) => {
            asking::deliver_held(realm, ctx, *task).await
        }
        Delivery::Kept(task) => {
            reviews::give(realm, ctx, *task).await
        }
    };
    delivered.map(|()| None)
}

async fn push(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
    push: Push,
) -> Result<()> {
    match push {
        Push::Place(agent) => place::send(realm, ctx, agent).await,
        Push::Reach(agent) => reach::push(realm, ctx, agent).await,
        Push::Drive(row) => {
            realm.drives.start(realm, ctx, row);
            Ok(())
        }
        Push::Copies(copied) => {
            documents::send(realm, ctx, copied).await
        }
    }
}

/// The work after an answer, as a task's future: the pushes in their
/// order, then the deliveries. Its type is named, so a commit that
/// sets it off may be made by a delivery.
pub(super) fn after_answer(
    realm: Arc<Realm>,
    ctx: Ctx<Realm>,
    deliveries: Vec<Queued>,
    pushes: Vec<Push>,
) -> BoxFuture<'static, ()> {
    Box::pin(async move {
        push_all(&realm, &ctx, pushes).await;
        deliver_all(&realm, &ctx, deliveries, When::AfterAnswer)
            .await;
    })
}

pub(super) async fn push_all(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
    pushes: Vec<Push>,
) -> Vec<anyhow::Error> {
    let mut failed = Vec::new();
    for made in pushes {
        let shown = format!("{made:?}");
        if let Err(error) = push(realm, ctx, made).await {
            tracing::warn!(push = shown, "a push failed: {error:#}");
            failed.push(error);
        }
    }
    failed
}
