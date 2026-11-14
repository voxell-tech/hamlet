//! The `task` table: what a chief's line or an agent's standing is
//! charged to (H12, C31). Outside this module the realm
//! names a task only as [`TaskId`], and only this module turns one
//! into or out of [`hamlet_messages::workspace::RealmTask`]. Taking
//! tasks out deletes this module, its table and `TaskId`: the
//! compiler then names every use, and the messages' `RealmTask`
//! fields go with `RealmTask` itself.

#![expect(
    unused_variables,
    dead_code,
    reason = "the scaffold: U2 fills this file"
)]

use anyhow::Result;
use hamlet_messages::realm::budget::Payer;
use hamlet_messages::workspace::{AgentNumber, RealmTask};
use serde::{Deserialize, Serialize};

use super::change::{Affected, Rows, Writer};

/// The realm's id of a task.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub(super) struct TaskId(pub u64);

impl TaskId {
    pub fn on_the_wire(self) -> RealmTask {
        todo!()
    }

    /// The task a message names, when the text is one of the
    /// realm's.
    pub fn of(task: &RealmTask) -> Option<Self> {
        todo!()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Origin {
    /// A chief's line.
    Chief,
    /// The task that pays for an agent's turns no line started.
    Standing,
}

/// The changes to `task`.
#[derive(Debug, PartialEq)]
pub(super) enum Change {
    Open {
        task: TaskId,
        origin: Origin,
        agent: AgentNumber,
        payer: Payer,
    },
    /// An agent's standing task, under an id of the next ones.
    Standing { agent: AgentNumber, payer: Payer },
}

pub(super) async fn apply(
    writer: &Writer<'_>,
    change: Change,
) -> Result<Affected> {
    match change {
        Change::Open {
            task,
            origin,
            agent,
            payer,
        } => open(writer, task, origin, agent, payer).await,
        Change::Standing { agent, payer } => {
            standing_task(writer, agent, payer).await
        }
    }
}

async fn open(
    writer: &Writer<'_>,
    task: TaskId,
    origin: Origin,
    agent: AgentNumber,
    payer: Payer,
) -> Result<Affected> {
    todo!()
}

async fn standing_task(
    writer: &Writer<'_>,
    agent: AgentNumber,
    payer: Payer,
) -> Result<Affected> {
    todo!()
}

/// The next task's id.
pub(super) async fn fresh(rows: &Rows<'_>) -> Result<TaskId> {
    todo!()
}

/// `agent`'s standing task.
pub(super) async fn standing(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<TaskId> {
    todo!()
}

/// Who pays for `task`. `None` for a task the realm does not hold.
pub(super) async fn payer(
    rows: &Rows<'_>,
    task: TaskId,
) -> Result<Option<Payer>> {
    todo!()
}
