//! Workers, numbered agents that run one turn for their starter: the
//! `worker` table, `StartWorker` and `WorkerDone`.

pub(super) mod errands;

use std::sync::Arc;

use anyhow::{Result, bail};
use hamlet_actor::Ctx;
use hamlet_messages::address::Agent;
use hamlet_messages::realm::AgentRefusal;
use hamlet_messages::realm::workers::{
    StartWorker, Started, WorkerDone, WorkerRefusal,
};
use hamlet_messages::sender::Client;
use hamlet_messages::workspace::AgentNumber;

use super::Realm;
use super::act::{Act, Decision, Unbuilt};
use super::calls::{Delivery, Made};
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

/// The changes that stop `starter`'s living workers, committed with
/// its end.
pub(super) async fn end_with(
    _rows: &Rows<'_>,
    _starter: AgentNumber,
) -> Result<Vec<change::Change>> {
    Ok(Vec::new())
}

/// Delivers the answer of `worker` to its starter.
pub(super) async fn answer(
    _realm: &Arc<Realm>,
    _ctx: &Ctx<Realm>,
    worker: AgentNumber,
) -> Result<()> {
    bail!(
        "not built yet: U7, so {:?} is not made",
        Delivery::WorkerAnswer(worker)
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    /// The workspace holds the worker's reach.
    Reach,
    /// `agent/<n>` is made with its brief.
    Create,
}

impl Steps for Step {
    const ALL: &'static [Self] = &[Self::Reach, Self::Create];

    fn name(self) -> &'static str {
        match self {
            Self::Reach => "reach",
            Self::Create => "create",
        }
    }
}

/// Does `step` of worker `n`'s start, and returns the changes
/// committed with its progress.
pub(super) async fn run(
    _cx: &Cx<'_>,
    _n: u64,
    _step: Step,
) -> Result<Vec<change::Change>> {
    bail!("not built yet: U7")
}

impl Act<Agent> for StartWorker {
    type Output = Started<AgentRefusal<WorkerRefusal>>;
    type Loaded = Unbuilt;
    type Done = Unbuilt;

    async fn load(
        &self,
        _by: &Agent,
        _rows: &Rows<'_>,
    ) -> Result<Unbuilt> {
        bail!("not built yet: U7")
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

impl Act<Client> for StartWorker {
    type Output = Started<WorkerRefusal>;
    type Loaded = Unbuilt;
    type Done = Unbuilt;

    async fn load(
        &self,
        _by: &Client,
        _rows: &Rows<'_>,
    ) -> Result<Unbuilt> {
        bail!("not built yet: U7")
    }

    fn decide(
        &self,
        _by: &Client,
        loaded: Unbuilt,
    ) -> Decision<Unbuilt, Self::Output> {
        match loaded {}
    }

    fn answer(&self, done: Unbuilt, _made: &Made) -> Self::Output {
        match done {}
    }
}

impl Act<Agent> for WorkerDone {
    type Output = ();
    type Loaded = Unbuilt;
    type Done = Unbuilt;

    async fn load(
        &self,
        _by: &Agent,
        _rows: &Rows<'_>,
    ) -> Result<Unbuilt> {
        bail!("not built yet: U7")
    }

    fn decide(
        &self,
        _by: &Agent,
        loaded: Unbuilt,
    ) -> Decision<Unbuilt, ()> {
        match loaded {}
    }

    fn answer(&self, done: Unbuilt, _made: &Made) {
        match done {}
    }
}
