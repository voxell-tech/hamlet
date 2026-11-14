//! The `step` table and its one walk: an operation that spans actors
//! is a row, driven from its last step done. A step's progress moves
//! by compare-and-set on the step before, in the transaction of that
//! step's changes, so a closed row stops it.
//!
//! An operation is a value of [`Kind`] and a step enum of its own.
//! [`drive`] matches on the kind with no `_` arm, so a new operation
//! does not build until it has a drive here.

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

/// What a row drives.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub(super) enum Kind {
    Hire,
}

/// A row's id: its kind and a number of that kind. A hire's number
/// is its agent's.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub(super) struct StepId {
    pub kind: Kind,
    pub n: u64,
}

/// One operation's steps, in the order they are done. The row
/// stores the name of the last one done.
pub(super) trait Steps:
    Copy + Eq + Send + Sync + 'static
{
    const ALL: &'static [Self];

    fn name(self) -> &'static str;
}

/// The changes to `step`.
#[derive(Debug, PartialEq)]
pub(super) enum Change {
    /// A new row with no step done. Its drive is reserved before the
    /// commit and started after it.
    Begin { row: StepId },
    /// `to` is done, when `from` was the last step done and the row
    /// is open.
    Advance {
        row: StepId,
        from: Option<&'static str>,
        to: &'static str,
    },
    /// The row is driven no further: an end of a `hiring` hire.
    Close { row: StepId },
}

/// Makes `change`. A `Begin` reports its row in `Affected::begun`.
/// An `Advance` that finds the row moved or closed changes nothing
/// and fails, which rolls back the step's other changes.
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

/// A row as it is read.
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

/// What a step works with: rows read with no transaction open, the
/// realm's peers, and a commit of its own before its call.
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
    /// they set off, waiting for each. A hire raises its wanted reach
    /// this way before it calls the workspace.
    pub async fn commit(
        &self,
        changes: Vec<change::Change>,
    ) -> Result<()> {
        todo!()
    }
}

/// Drives `row` from its last step done. Runs on a task of the
/// realm's; a step that fails is logged and its row stays where it
/// is, for the realm's next start or the caller's repeat.
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

/// Runs each step of `S` after the row's last one done. A step's
/// changes and its progress commit together; the progress is a
/// compare-and-set on the step before, so an `End` that closed the
/// row, or another drive, stops this one.
async fn walk<S: Steps>(
    cx: &Cx<'_>,
    id: StepId,
    run: impl AsyncFn(&Cx<'_>, S) -> Result<Vec<change::Change>>,
) -> Result<()> {
    todo!()
}

/// The rows being driven. A row has one drive at a time: its entry
/// is taken under the lock, and its drive removes it at its end.
#[derive(Default)]
pub(super) struct Drives {
    running: Arc<Mutex<HashMap<StepId, watch::Receiver<()>>>>,
}

/// The entries of rows about to be committed. Dropped without
/// [`Reserved::start`], as when the commit fails, it gives them back.
#[must_use]
pub(super) struct Reserved {
    rows: Vec<(StepId, watch::Sender<()>)>,
    running: Arc<Mutex<HashMap<StepId, watch::Receiver<()>>>>,
}

impl Reserved {
    /// Drives each row on a task of the realm's. Waits for nothing.
    pub fn start(self, realm: &Arc<Realm>, ctx: &Ctx<Realm>) {
        todo!()
    }
}

impl Drives {
    /// Takes the entry of each of `rows` that no drive holds. A
    /// commit that begins a row calls it before it commits, so no
    /// action finds a `hiring` row with no drive.
    pub fn reserve(&self, rows: &[StepId]) -> Reserved {
        todo!()
    }

    /// Drives `row`, unless a drive holds it: a repeat of a hire, and
    /// the realm's start.
    pub fn start(
        &self,
        realm: &Arc<Realm>,
        ctx: &Ctx<Realm>,
        row: StepId,
    ) {
        self.reserve(&[row]).start(realm, ctx);
    }

    /// Waits until `row`'s drive ends, at most `within`. Returns
    /// whether one ended in time, and `false` at once when none runs.
    pub async fn finished(
        &self,
        row: StepId,
        within: Duration,
    ) -> bool {
        todo!()
    }
}

/// Starts a drive of every open row with a step left. Reads rows and
/// registers tasks; waits for no call, since the start has a
/// deadline.
pub(super) async fn drive_undone(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
) -> Result<()> {
    todo!()
}
