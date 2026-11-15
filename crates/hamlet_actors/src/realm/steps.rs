//! The `step` table and its one walk: an operation that spans actors
//! is a row, driven from its last step done.

#![expect(
    unused_variables,
    dead_code,
    reason = "the scaffold: U2 fills this file"
)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use hamlet_actor::Ctx;
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use super::change::{self, Affected, Rows, Writer};
use super::peer::Peers;
use super::{Realm, hire};

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub(super) enum Kind {
    Hire,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub(super) struct StepId {
    pub kind: Kind,
    pub n: u64,
}

/// One operation's steps, in the order they are done.
pub(super) trait Steps:
    Copy + Eq + Send + Sync + 'static
{
    const ALL: &'static [Self];

    fn name(self) -> &'static str;
}

#[derive(Debug, PartialEq)]
pub(super) enum Change {
    /// A new row with no step done.
    Begin { row: StepId },
    /// `to` is done, when `from` was the last step done and the row
    /// is open.
    Advance {
        row: StepId,
        from: Option<&'static str>,
        to: &'static str,
    },
    /// The row is driven no further.
    Close { row: StepId },
}

/// An `Advance` that finds the row moved or closed changes nothing
/// and fails.
pub(super) async fn apply(
    writer: &Writer<'_>,
    change: Change,
) -> Result<Affected> {
    match change {
        Change::Begin { row } => begin(writer, row).await,
        Change::Advance { row, from, to } => {
            advance(writer, row, from, to).await
        }
        Change::Close { row } => close(writer, row).await,
    }
}

async fn begin(writer: &Writer<'_>, row: StepId) -> Result<Affected> {
    todo!()
}

async fn advance(
    writer: &Writer<'_>,
    row: StepId,
    from: Option<&'static str>,
    to: &'static str,
) -> Result<Affected> {
    todo!()
}

async fn close(writer: &Writer<'_>, row: StepId) -> Result<Affected> {
    todo!()
}

pub(super) struct Row {
    pub id: StepId,
    pub done: Option<String>,
    pub closed: bool,
}

pub(super) async fn row(
    rows: &Rows<'_>,
    id: StepId,
) -> Result<Option<Row>> {
    todo!()
}

pub(super) struct Cx<'a> {
    pub realm: &'a Arc<Realm>,
    pub ctx: &'a Ctx<Realm>,
}

impl Cx<'_> {
    pub fn rows(&self) -> Result<Rows<'_>> {
        Rows::outside(self.ctx)
    }

    pub fn peers(&self) -> &Peers {
        &self.realm.peers
    }

    /// Commits `changes` before the step's call, and makes the pushes
    /// they set off, waiting for each.
    pub async fn commit(
        &self,
        changes: Vec<change::Change>,
    ) -> Result<()> {
        todo!()
    }
}

async fn drive(realm: Arc<Realm>, ctx: Ctx<Realm>, row: StepId) {
    let cx = Cx {
        realm: &realm,
        ctx: &ctx,
    };
    let driven = match row.kind {
        Kind::Hire => {
            walk(&cx, row, async |cx: &Cx<'_>, step| {
                hire::run(cx, row.n, step).await
            })
            .await
        }
    };
    if let Err(error) = driven {
        tracing::warn!(?row, "a step failed: {error:#}");
    }
}

/// Runs each step of `S` after the row's last one done.
async fn walk<S: Steps>(
    cx: &Cx<'_>,
    id: StepId,
    run: impl AsyncFn(&Cx<'_>, S) -> Result<Vec<change::Change>>,
) -> Result<()> {
    todo!()
}

#[derive(Default)]
pub(super) struct Drives {
    running: Arc<Mutex<HashMap<StepId, watch::Receiver<()>>>>,
}

/// The entries of rows about to be committed.
#[must_use]
pub(super) struct Reserved {
    rows: Vec<(StepId, watch::Sender<()>)>,
    running: Arc<Mutex<HashMap<StepId, watch::Receiver<()>>>>,
}

impl Reserved {
    /// Drives each row on a task of the realm's.
    pub fn start(self, realm: &Arc<Realm>, ctx: &Ctx<Realm>) {
        todo!()
    }
}

impl Drives {
    /// Takes the entry of each of `rows` that no drive holds.
    pub fn reserve(&self, rows: &[StepId]) -> Reserved {
        todo!()
    }

    pub fn start(
        &self,
        realm: &Arc<Realm>,
        ctx: &Ctx<Realm>,
        row: StepId,
    ) {
        self.reserve(&[row]).start(realm, ctx);
    }

    /// Waits until `row`'s drive ends, at most `within`, and at once
    /// when none runs.
    pub async fn finished(&self, row: StepId, within: Duration) {
        todo!()
    }
}

/// Starts a drive of every open row with a step left.
pub(super) async fn drive_undone(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
) -> Result<()> {
    todo!()
}
