//! The calls the realm makes after a commit.

use std::sync::Arc;

use anyhow::Result;
use futures::future::BoxFuture;
use hamlet_actor::{Ctx, KeepAwakeRegion};
use hamlet_messages::address::Agent;
use hamlet_messages::agent::{Input, Submit};
use hamlet_messages::realm::LetterId;
use hamlet_messages::realm::world::WaitId;
use hamlet_messages::workspace::AgentNumber;
use serde::{Deserialize, Serialize};

use super::change::{Affected, Rows};
use super::lanes::{self, Queued};
use super::oversight::asking;
use super::peer::{self, When};
use super::tasks::TaskId;
use super::{
    Realm, documents, letters, place, reach, reviews, steps, tree,
    waits, workers,
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

    pub fn before_answer(delivery: Delivery) -> Self {
        Self {
            before_answer: vec![delivery],
            after_answer: Vec::new(),
        }
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
}

impl Made {
    pub fn all_taken(&self) -> bool {
        self.failed.is_empty()
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
    let made = futures::future::join_all(
        deliveries
            .into_iter()
            .map(|queued| deliver(realm, ctx, queued, when)),
    )
    .await;
    Made {
        failed: made.into_iter().filter_map(Result::err).collect(),
    }
}

async fn deliver(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
    Queued { delivery, mut turn }: Queued,
    when: When,
) -> Result<()> {
    let owed = delivery
        .owed()
        .then(|| (delivery.clone(), ctx.keep_awake_region()));
    turn.come().await;
    let delivered = make(realm, ctx, delivery, when).await;
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
    delivered
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
            make(realm, ctx, delivery, When::AfterAnswer).await
        }
    })
    .await;
}

async fn make(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
    delivery: Delivery,
    when: When,
) -> Result<()> {
    let shown = format!("{delivery:?}");
    let delivered = match delivery {
        Delivery::Line { to, submit }
        | Delivery::ParcelRefused { to, submit } => {
            realm.peers.call(&Agent(to), &submit, when).await
        }
        Delivery::Letter(_) | Delivery::Answer(_) => {
            letters::deliver(realm, ctx, delivery).await
        }
        Delivery::ChiefAnswer(wait) => {
            waits::deliver(realm, ctx, wait).await
        }
        Delivery::Notice(wait) => {
            waits::notice(realm, ctx, wait).await
        }
        Delivery::WorkerAnswer(worker) => {
            workers::answer(realm, ctx, worker).await
        }
        Delivery::Held(task) => {
            asking::deliver_held(realm, ctx, task).await
        }
        Delivery::Kept(task) => reviews::give(realm, ctx, task).await,
    };
    if let Err(error) = &delivered {
        tracing::warn!(
            delivery = shown,
            "a delivery failed: {error:#}"
        );
    }
    delivered
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
) -> usize {
    let mut failed = 0;
    for made in pushes {
        let shown = format!("{made:?}");
        if let Err(error) = push(realm, ctx, made).await {
            tracing::warn!(push = shown, "a push failed: {error:#}");
            failed += 1;
        }
    }
    failed
}
