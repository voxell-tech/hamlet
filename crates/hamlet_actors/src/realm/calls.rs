//! The calls the realm makes after a commit.

use std::sync::Arc;

use anyhow::Result;
use hamlet_actor::Ctx;
use hamlet_messages::address::Agent;
use hamlet_messages::agent::Submit;
use hamlet_messages::realm::LetterId;
use hamlet_messages::realm::world::WaitId;
use hamlet_messages::workspace::AgentNumber;
use serde::{Deserialize, Serialize};

use super::change::Affected;
use super::peer::When;
use super::{Realm, letters, place, reach, steps, waits};

/// A call that carries an act's content.
#[derive(Debug, PartialEq)]
pub(super) enum Delivery {
    /// A chief's line.
    Line { to: AgentNumber, submit: Submit },
    /// The letter or the question `0`, from its row, to its receiver.
    Letter(LetterId),
    /// The answer to the question `0`, from its row, to the asker's
    /// conversation.
    Answer(LetterId),
    /// The chief's answer to the wait `0`, from its row, to the
    /// workspace.
    ChiefAnswer(WaitId),
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
}

impl Push {
    /// The pushes of a commit, in the order they are made: the
    /// drives, then the reaches, then the places.
    pub fn of(affected: &Affected) -> Vec<Self> {
        let drives = affected.begun.iter().copied().map(Self::Drive);
        let reaches =
            affected.reaches.iter().copied().map(Self::Reach);
        let places = affected.places.iter().copied().map(Self::Place);
        drives.chain(reaches).chain(places).collect()
    }
}

pub(super) async fn deliver_all(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
    deliveries: Vec<Delivery>,
    when: When,
) -> Made {
    let mut made = Made::default();
    for delivery in deliveries {
        let shown = format!("{delivery:?}");
        let delivered = match delivery {
            Delivery::Line { to, submit } => {
                realm.peers.call(&Agent(to), &submit, when).await
            }
            Delivery::Letter(_) | Delivery::Answer(_) => {
                letters::deliver(realm, ctx, delivery).await
            }
            Delivery::ChiefAnswer(wait) => {
                waits::deliver(realm, ctx, wait).await
            }
        };
        if let Err(error) = delivered {
            tracing::warn!(
                delivery = shown,
                "a delivery failed: {error:#}"
            );
            made.failed.push(error);
        }
    }
    made
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
    }
}

/// The work after an answer: the pushes in their order, then the
/// deliveries.
pub(super) async fn after_answer(
    realm: Arc<Realm>,
    ctx: Ctx<Realm>,
    deliveries: Vec<Delivery>,
    pushes: Vec<Push>,
) {
    push_all(&realm, &ctx, pushes).await;
    deliver_all(&realm, &ctx, deliveries, When::AfterAnswer).await;
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
