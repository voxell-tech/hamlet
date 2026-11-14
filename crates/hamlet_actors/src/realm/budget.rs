//! Spending: the `charge` and `budget_reset` tables, and the check
//! before a model call (H8). Outside this module the realm calls only
//! `spent` (the world) and `Change::Road` (a laid road's charge).
//! Taking budgets out deletes this module, its two tables, those two
//! uses, `hamlet_messages::realm::budget` and the agent's two calls;
//! a model call then waits on no realm.

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

/// The changes to this module's tables.
#[derive(Debug, PartialEq)]
pub(super) enum Change {
    /// One model call's cost and time, once per (agent, call): its
    /// statement inserts or does nothing, so a repeat counts once
    /// with no row read first.
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

/// Makes `change`. Spending is seen by a client, as `Part::Spend`.
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
/// reset, as of `rows.at()`, with the budget it was counted against.
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

/// What a check before a model call decides over.
pub(super) struct Spending {
    /// How the caller ended, when it did.
    left: Option<Left>,
    /// The spending of the task's payer. `None` for a task the realm
    /// does not hold.
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

    /// A read: every outcome is an `Answer`, and nothing is written.
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
    /// The payer of the task.
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
    /// The hamlet that pays, when the payer is one.
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
