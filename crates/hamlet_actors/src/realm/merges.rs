//! A chief's `/merge` and a lead's `merge_hires`, a chief's
//! `/restore` of an ended hire or of one merged away, and the
//! `merge` and `split` rows that carry a merge's files.

use anyhow::{Result, bail};
use hamlet_messages::address::Agent;
use hamlet_messages::realm::hires::{
    Merge, MergeRefusal, MergedTo, Restore, Restored,
};
use hamlet_messages::realm::{Done, LeadRefusal};
use hamlet_messages::sender::Client;

use super::act::unbuilt;
use super::calls::Calls;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    Retire,
    Merge,
    Fill,
    Inbox,
}

impl Steps for Step {
    const ALL: &'static [Self] =
        &[Self::Retire, Self::Merge, Self::Fill, Self::Inbox];

    fn name(self) -> &'static str {
        match self {
            Self::Retire => "retire",
            Self::Merge => "merge",
            Self::Fill => "fill",
            Self::Inbox => "inbox",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SplitStep {
    Unmerge,
    Fill,
}

impl Steps for SplitStep {
    const ALL: &'static [Self] = &[Self::Unmerge, Self::Fill];

    fn name(self) -> &'static str {
        match self {
            Self::Unmerge => "unmerge",
            Self::Fill => "fill",
        }
    }
}

unbuilt!(Client:
    Merge => Done<MergeRefusal>,
    Restore => Restored,
);
unbuilt!(Agent: Merge => MergedTo<LeadRefusal<MergeRefusal>>);

pub(super) async fn run(
    _cx: &Cx<'_>,
    _n: u64,
    _step: Step,
) -> Result<Vec<change::Change>> {
    bail!("not implemented")
}

pub(super) async fn failed(
    _rows: &Rows<'_>,
    _n: u64,
) -> Result<(Vec<change::Change>, Calls)> {
    bail!("not implemented")
}

pub(super) async fn split(
    _cx: &Cx<'_>,
    _n: u64,
    _step: SplitStep,
) -> Result<Vec<change::Change>> {
    bail!("not implemented")
}

pub(super) async fn split_failed(
    _rows: &Rows<'_>,
    _n: u64,
) -> Result<(Vec<change::Change>, Calls)> {
    bail!("not implemented")
}
