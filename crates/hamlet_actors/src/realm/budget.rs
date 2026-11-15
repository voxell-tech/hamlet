//! Spending: the `charge` and `budget_reset` tables, and the check
//! before a model call (H8).

#![expect(
    unused_variables,
    dead_code,
    reason = "the scaffold: U7 fills this file"
)]

use anyhow::Result;
use hamlet_messages::address::Agent;
use hamlet_messages::realm::budget::{
    Charge, MaySpend, Payer, ResetBudget, SpendRefusal,
};
use hamlet_messages::realm::{Done, HamletRefusal, Left};
use hamlet_messages::sender::Client;
use hamlet_messages::workspace::{AgentNumber, CallId};

use super::act::{Act, Decision};
use super::calls::Made;
use super::change::{Affected, Rows, Writer};
use super::chiefs;

#[derive(Debug, PartialEq)]
pub(super) enum Change {
    /// One model call's cost and time, once per (agent, call).
    Charge {
        agent: AgentNumber,
        call: CallId,
        payer: Payer,
        micros: u32,
        took_ms: u32,
    },
    /// A laid road's price, charged once (H3).
    Road {
        a: AgentNumber,
        b: AgentNumber,
        payer: Payer,
    },
    Reset {
        payer: Payer,
    },
}

pub(super) async fn apply(
    writer: &Writer<'_>,
    change: Change,
) -> Result<Affected> {
    match change {
        Change::Charge {
            agent,
            call,
            payer,
            micros,
            took_ms,
        } => {
            charge(writer, agent, call, payer, micros, took_ms).await
        }
        Change::Road { a, b, payer } => {
            road(writer, a, b, payer).await
        }
        Change::Reset { payer } => reset(writer, payer).await,
    }
}

async fn charge(
    writer: &Writer<'_>,
    agent: AgentNumber,
    call: CallId,
    payer: Payer,
    micros: u32,
    took_ms: u32,
) -> Result<Affected> {
    todo!()
}

async fn road(
    writer: &Writer<'_>,
    a: AgentNumber,
    b: AgentNumber,
    payer: Payer,
) -> Result<Affected> {
    todo!()
}

async fn reset(
    writer: &Writer<'_>,
    payer: Payer,
) -> Result<Affected> {
    todo!()
}

/// A payer's spending since its period began and since its last
/// reset, as of `rows.at()`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Spent {
    pub payer: Payer,
    pub micros: u64,
    pub took_ms: u64,
    pub budget: chiefs::limits::Budget,
}

pub(super) async fn spent(
    rows: &Rows<'_>,
    payer: &Payer,
) -> Result<Spent> {
    todo!()
}

pub(super) struct Spending {
    left: Option<Left>,
    /// `None` for a task the realm does not hold.
    spent: Option<Spent>,
}

impl Act<Agent> for MaySpend {
    type Output = Done<SpendRefusal>;
    type Loaded = Spending;
    type Done = ();

    async fn load(
        &self,
        by: &Agent,
        rows: &Rows<'_>,
    ) -> Result<Spending> {
        todo!()
    }

    fn decide(
        &self,
        by: &Agent,
        spending: Spending,
    ) -> Decision<(), Self::Output> {
        todo!()
    }

    fn answer(&self, (): (), made: &Made) -> Self::Output {
        todo!()
    }
}

impl Act<Agent> for Charge {
    type Output = ();
    type Loaded = Option<Payer>;
    type Done = ();

    async fn load(
        &self,
        by: &Agent,
        rows: &Rows<'_>,
    ) -> Result<Option<Payer>> {
        todo!()
    }

    fn decide(
        &self,
        by: &Agent,
        payer: Option<Payer>,
    ) -> Decision<(), ()> {
        todo!()
    }

    fn answer(&self, (): (), made: &Made) {
        todo!()
    }
}

impl Act<Client> for ResetBudget {
    type Output = Done<HamletRefusal>;
    type Loaded = Option<Option<chiefs::Hamlet>>;
    type Done = ();

    async fn load(
        &self,
        by: &Client,
        rows: &Rows<'_>,
    ) -> Result<Option<Option<chiefs::Hamlet>>> {
        todo!()
    }

    fn decide(
        &self,
        by: &Client,
        hamlet: Option<Option<chiefs::Hamlet>>,
    ) -> Decision<(), Self::Output> {
        todo!()
    }

    fn answer(&self, (): (), made: &Made) -> Self::Output {
        todo!()
    }
}
