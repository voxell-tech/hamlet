//! What a hire and everyone under it may spend: the `allowance`
//! table, `SetAllowance` and `ReadBudget`.

use anyhow::{Result, bail};
use hamlet_messages::address::Agent;
use hamlet_messages::realm::budget::{
    Budget, BudgetRead, ReadBudget, ReadBudgetRefusal,
};
use hamlet_messages::realm::workers::{
    AllowanceRefusal, SetAllowance,
};
use hamlet_messages::realm::{AgentRefusal, Done};
use hamlet_messages::workspace::AgentNumber;

use super::act::{Act, Decision, Unbuilt};
use super::callers::Lead;
use super::calls::Made;
use super::change::{Affected, At, Rows, Writer};

#[derive(Debug, PartialEq)]
pub(super) enum Change {}

pub(super) async fn apply(
    _writer: &Writer<'_>,
    change: Change,
) -> Result<Affected> {
    match change {}
}

/// The allowance `agent` or one of its leads has spent, as of `at`.
pub(super) async fn over(
    _rows: &Rows<'_>,
    _agent: AgentNumber,
    _at: At,
) -> Result<Option<Budget>> {
    Ok(None)
}

impl<By: Lead> Act<By> for SetAllowance {
    type Output = Done<By::Why<AllowanceRefusal>>;
    type Loaded = Unbuilt;
    type Done = Unbuilt;

    async fn load(
        &self,
        _by: &By,
        _rows: &Rows<'_>,
    ) -> Result<Unbuilt> {
        bail!("not built yet: U8")
    }

    fn decide(
        &self,
        _by: &By,
        loaded: Unbuilt,
    ) -> Decision<Unbuilt, Self::Output> {
        match loaded {}
    }

    fn answer(&self, done: Unbuilt, _made: &Made) -> Self::Output {
        match done {}
    }
}

impl Act<Agent> for ReadBudget {
    type Output = BudgetRead<AgentRefusal<ReadBudgetRefusal>>;
    type Loaded = Unbuilt;
    type Done = Unbuilt;

    async fn load(
        &self,
        _by: &Agent,
        _rows: &Rows<'_>,
    ) -> Result<Unbuilt> {
        bail!("not built yet: U8")
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
