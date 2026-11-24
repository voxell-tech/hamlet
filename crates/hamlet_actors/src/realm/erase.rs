//! A chief's `/erase` of an ended hire, and the `erase` row that
//! drops its branch and removes its agent.

use anyhow::{Result, bail};
use hamlet_messages::realm::Done;
use hamlet_messages::realm::hires::{EndedRefusal, EraseHire};
use hamlet_messages::sender::Client;

use super::act::unbuilt;
use super::calls::Calls;
use super::change::{self, Rows};
use super::steps::{Cx, Steps};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    Branch,
    Agent,
}

impl Steps for Step {
    const ALL: &'static [Self] = &[Self::Branch, Self::Agent];

    fn name(self) -> &'static str {
        match self {
            Self::Branch => "branch",
            Self::Agent => "agent",
        }
    }
}

unbuilt!(Client: EraseHire => Done<EndedRefusal>);

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
