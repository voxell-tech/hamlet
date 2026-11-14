//! The waits for a chief: the `wait` table, `NeedsChief` and the
//! chief's `Answer` (H11, H34).

#![expect(
    unused_variables,
    dead_code,
    reason = "the scaffold: U8 fills this file"
)]

use std::sync::Arc;

use anyhow::Result;
use hamlet_actor::Ctx;
use hamlet_messages::address::Workspace;
use hamlet_messages::realm::client::{Answer, AnswerRefusal};
use hamlet_messages::realm::{Done, NeedsChief};
use hamlet_messages::sender::Client;
use hamlet_messages::workspace::AgentNumber;
use serde::{Deserialize, Serialize};

use super::act::{Act, Decision};
use super::calls::{Calls, Made};
use super::change::{Affected, Rows, Writer};
use super::{Realm, not_built};

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub(super) struct WaitId(pub u64);

/// How long an allow holds. The workspace hears only allow or
/// refuse, and every allow acts once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Scope {
    Once,
    Conversation,
    Always,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Choice {
    Allow(Scope),
    Refuse { reason: Option<String> },
}

/// The changes to `wait`.
#[derive(Debug, PartialEq)]
pub(super) enum Change {
    /// A wait opens, once per (agent, call): its statement inserts or
    /// does nothing, so a repeat of `NeedsChief` writes nothing more.
    /// The row holds the realm's own columns, read from the message.
    Open {
        asked: NeedsChief,
    },
    Close {
        wait: WaitId,
        choice: Choice,
    },
    /// Every open wait of an agent that ended closes, as lapsed.
    Lapse {
        agent: AgentNumber,
    },
}

/// Makes `change`. A client sees each, as `Part::Waits`.
pub(super) async fn apply(
    writer: &Writer<'_>,
    change: Change,
) -> Result<Affected> {
    match change {
        Change::Open { asked } => open(writer, asked).await,
        Change::Close { wait, choice } => {
            close(writer, wait, choice).await
        }
        Change::Lapse { agent } => lapse(writer, agent).await,
    }
}

async fn open(
    writer: &Writer<'_>,
    asked: NeedsChief,
) -> Result<Affected> {
    todo!()
}

async fn close(
    writer: &Writer<'_>,
    wait: WaitId,
    choice: Choice,
) -> Result<Affected> {
    todo!()
}

async fn lapse(
    writer: &Writer<'_>,
    agent: AgentNumber,
) -> Result<Affected> {
    Ok(Affected::default())
}

/// Calls the workspace with the chief's answer to `wait`, from its
/// row: allow or refuse. The workspace answers `NoWait` to a second.
pub(super) async fn deliver(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
    wait: WaitId,
) -> Result<()> {
    Err(not_built("U8"))
}

/// A call that waits for a chief, from the workspace, which holds the
/// agent's call meanwhile. It arrives bare: (agent, call) is its key.
impl Act<Workspace> for NeedsChief {
    type Output = ();
    type Loaded = ();
    type Done = ();

    async fn load(
        &self,
        by: &Workspace,
        rows: &Rows<'_>,
    ) -> Result<()> {
        todo!()
    }

    fn decide(&self, by: &Workspace, (): ()) -> Decision<(), ()> {
        todo!()
    }

    fn answer(&self, (): (), made: &Made) {
        todo!()
    }
}

/// A wait as its row is.
pub(super) struct Waiting {
    pub wait: WaitId,
    pub agent: AgentNumber,
    pub open: bool,
}

impl Act<Client> for Answer {
    type Output = Done<AnswerRefusal>;
    type Loaded = Option<Waiting>;
    type Done = WaitId;

    async fn load(
        &self,
        by: &Client,
        rows: &Rows<'_>,
    ) -> Result<Option<Waiting>> {
        todo!()
    }

    fn decide(
        &self,
        by: &Client,
        waiting: Option<Waiting>,
    ) -> Decision<WaitId, Self::Output> {
        todo!()
    }

    fn deliveries(&self, by: &Client, wait: &WaitId) -> Calls {
        todo!()
    }

    fn answer(&self, wait: WaitId, made: &Made) -> Self::Output {
        todo!()
    }
}
