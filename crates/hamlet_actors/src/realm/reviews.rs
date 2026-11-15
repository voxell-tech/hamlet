//! A lead's review of a chief's line to its hire, before the hire
//! receives it: the `review` table and its deadline.

use anyhow::{Result, bail};
use hamlet_messages::agent::Submit;
use hamlet_messages::realm::workers::Ended;

use super::change::{self, Affected, Rows, Writer};
use super::steps::{Cx, Steps};
use super::tasks::TaskId;
use super::tree::Lineage;

#[derive(Debug, PartialEq)]
pub(super) enum Change {}

pub(super) async fn apply(
    _writer: &Writer<'_>,
    change: Change,
) -> Result<Affected> {
    match change {}
}

/// The changes that hold `line`, a chief's line on `task` to the hire
/// of `lineage`, for its lead's review instead of delivering it.
pub(super) async fn intercept(
    _rows: &Rows<'_>,
    _lineage: &Lineage,
    _task: TaskId,
    _line: &Submit,
) -> Result<Option<Vec<change::Change>>> {
    Ok(None)
}

/// The changes the end of the review of `task`'s line makes.
pub(super) fn ended(
    _task: TaskId,
    _ended: &Ended,
) -> Vec<change::Change> {
    Vec::new()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    /// The review's deadline passed, or its errand ended first.
    Deadline,
}

impl Steps for Step {
    const ALL: &'static [Self] = &[Self::Deadline];

    fn name(self) -> &'static str {
        match self {
            Self::Deadline => "deadline",
        }
    }
}

/// Does `step` of the review of task `n`'s line, and returns the
/// changes committed with its progress.
pub(super) async fn run(
    _cx: &Cx<'_>,
    _n: u64,
    _step: Step,
) -> Result<Vec<change::Change>> {
    bail!("not built yet: U6")
}
