//! Parcels, the files a letter or a question sends: the `parcel` and
//! `parcel_file` tables, their steps, and `FetchPart`.

use std::sync::Arc;

use anyhow::{Result, bail};
use hamlet_actor::{Ctx, Handler};
use hamlet_messages::address::Agent;
use hamlet_messages::realm::client::Answer;
use hamlet_messages::realm::parcels::{
    FetchPart, ParcelFile, Part, PartRefusal,
};
use hamlet_messages::realm::world::WaitId;
use hamlet_messages::realm::{Enclosed, LetterId};
use hamlet_messages::sender::sender;
use hamlet_protocol::client::Choice;

use super::Realm;
use super::change::{self, Affected, Rows, Writer};
use super::steps::{Cx, Steps};
use super::waits::Ruling;

#[derive(Debug, PartialEq)]
pub(super) enum Change {}

pub(super) async fn apply(
    _writer: &Writer<'_>,
    change: Change,
) -> Result<Affected> {
    match change {}
}

/// The changes that send the files under `paths` with `letter`: its
/// parcel and the parcel's steps, which deliver the letter when the
/// files are copied.
pub(super) async fn start(
    _rows: &Rows<'_>,
    _letter: LetterId,
    _paths: &[Enclosed],
) -> Result<Option<Vec<change::Change>>> {
    Ok(None)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    /// The workspace measured the files.
    Measure,
    /// The files are copied into the realm.
    Copy,
    /// The letter is delivered with its files.
    Deliver,
}

impl Steps for Step {
    const ALL: &'static [Self] =
        &[Self::Measure, Self::Copy, Self::Deliver];

    fn name(self) -> &'static str {
        match self {
            Self::Measure => "measure",
            Self::Copy => "copy",
            Self::Deliver => "deliver",
        }
    }
}

/// Does `step` of parcel `n`, and returns the changes committed with
/// its progress.
pub(super) async fn run(
    _cx: &Cx<'_>,
    _n: u64,
    _step: Step,
) -> Result<Vec<change::Change>> {
    bail!("not built yet: U13")
}

/// Reads the realm's store, not its rows, so it runs outside `Act`.
impl Handler<FetchPart> for Realm {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        fetch: FetchPart,
    ) -> Result<Part<PartRefusal>> {
        let Agent(_) = sender(&ctx, &fetch)?;
        bail!("not built yet: U13")
    }
}

/// The files of `letter`'s parcel, as its receiver is told of them:
/// none until they are copied.
pub(super) async fn files(
    _rows: &Rows<'_>,
    _letter: LetterId,
) -> Result<Vec<ParcelFile>> {
    Ok(Vec::new())
}

/// Removes the realm's files no unswept parcel names, and the
/// temporary files no copy is writing.
pub(super) async fn sweep(
    _realm: &Arc<Realm>,
    _ctx: &Ctx<Realm>,
) -> Result<()> {
    Ok(())
}

/// What a parcel's send's wait adds to the chief's `answer` on wait
/// `wait`.
pub(super) async fn ruling(
    _rows: &Rows<'_>,
    wait: WaitId,
    answer: &Answer,
) -> Result<Ruling> {
    if answer.0.choice == Choice::Allow {
        bail!(
            "not built yet: U13, so the send wait {} is not allowed",
            wait.0
        );
    }
    Ok(Ruling::default())
}
