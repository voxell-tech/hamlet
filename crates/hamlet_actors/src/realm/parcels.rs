//! Parcels, the files a letter or a question sends: the `parcel` and
//! `parcel_file` tables, their steps, and `FetchPart`.

use anyhow::{Result, bail};
use hamlet_messages::address::Agent;
use hamlet_messages::realm::parcels::{FetchPart, Part, PartRefusal};
use hamlet_messages::realm::{Enclosed, LetterId};

use super::act::{Act, Decision, Unbuilt};
use super::calls::Made;
use super::change::{self, Affected, Rows, Writer};
use super::steps::{Cx, Steps};

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

impl Act<Agent> for FetchPart {
    type Output = Part<PartRefusal>;
    type Loaded = Unbuilt;
    type Done = Unbuilt;

    async fn load(
        &self,
        _by: &Agent,
        _rows: &Rows<'_>,
    ) -> Result<Unbuilt> {
        bail!("not built yet: U13")
    }

    fn decide(
        &self,
        _by: &Agent,
        loaded: Unbuilt,
    ) -> Decision<Unbuilt, Self::Output> {
        match loaded {}
    }

    fn answer(&self, done: Unbuilt, _made: &Made) -> Self::Output {
        match done {}
    }
}
