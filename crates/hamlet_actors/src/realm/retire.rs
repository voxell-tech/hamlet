//! The `retire` row: an ended hire's agent stops its turn and saves
//! its files to its branch, so a later merge or restore reads them.

use anyhow::{Result, bail};

use super::calls::Calls;
use super::change::{self, Rows};
use super::steps::{Cx, Steps};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    Retire,
}

impl Steps for Step {
    const ALL: &'static [Self] = &[Self::Retire];

    fn name(self) -> &'static str {
        match self {
            Self::Retire => "retire",
        }
    }
}

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
