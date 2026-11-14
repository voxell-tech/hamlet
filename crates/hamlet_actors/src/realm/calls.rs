//! The calls the realm makes after a commit: a delivery carries an
//! act's own content, and a push carries versioned state that a
//! change moved.

#![expect(
    unused_variables,
    dead_code,
    reason = "the scaffold: U2 fills this file"
)]

use std::sync::Arc;

use anyhow::Result;
use hamlet_actor::Ctx;
use hamlet_messages::address::Agent;
use hamlet_messages::agent::Submit;
use hamlet_messages::realm::LetterId;
use hamlet_messages::workspace::AgentNumber;
use serde::{Deserialize, Serialize};

use super::change::Affected;
use super::peer::When;
use super::{Realm, letters, place, reach, steps, waits};

/// A call that carries an act's content. An act names its
/// deliveries from what it recorded, so a repeat makes them again.
#[derive(Debug, PartialEq)]
pub(super) enum Delivery {
    /// A chief's line, which no row keeps: the act holds it.
    Line { to: AgentNumber, submit: Submit },
    /// The letter or the question `0`, from its row, to its receiver.
    Letter(LetterId),
    /// The answer to the question `0`, from its row, to the asker's
    /// conversation.
    Answer(LetterId),
    /// The chief's answer to the wait `0`, from its row, to the
    /// workspace.
    ChiefAnswer(waits::WaitId),
}

/// The deliveries of one act.
#[derive(Debug, Default, PartialEq)]
pub(super) struct Calls {
    /// Made before the act answers, so its answer tells how they
    /// went.
    pub before_answer: Vec<Delivery>,
    /// Made after the act answered.
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

/// How the deliveries before an answer went.
#[derive(Debug, Default)]
pub(super) struct Made {
    failed: Vec<anyhow::Error>,
}

impl Made {
    /// Whether every one was taken. True when there was none.
    pub fn all_taken(&self) -> bool {
        self.failed.is_empty()
    }
}

/// State a commit moved, to carry to the actor that keeps a copy.
/// It names rows, so the call is built from them as they are when
/// it is made. A request's row keeps its pushes for a repeat.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum Push {
    /// The agent's place, to the agent.
    Place(AgentNumber),
    /// The agent's reach, to the workspace, whose answer is
    /// committed.
    Reach(AgentNumber),
    /// The steps of a row. The commit that began the row started its
    /// drive already, so this one is for a repeat.
    Drive(steps::StepId),
}

impl Push {
    /// The pushes of a commit, in the order they are made: the
    /// drives, which wait for nothing, then the reaches, then the
    /// places.
    pub fn of(affected: &Affected) -> Vec<Self> {
        todo!()
    }
}

/// Makes `deliveries` in order and tells which failed. A failed one
/// is logged and not made again.
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

/// Makes `push`.
pub(super) async fn push(
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
/// deliveries. Each failure is logged, and nothing is made again.
pub(super) async fn after_answer(
    realm: Arc<Realm>,
    ctx: Ctx<Realm>,
    deliveries: Vec<Delivery>,
    pushes: Vec<Push>,
) {
    todo!()
}
