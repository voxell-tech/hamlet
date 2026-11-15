//! The realm's row, the chief and the hamlets: the `realm`, `chief`,
//! `hamlet` and `hamlet_chief` tables, and founding a hamlet.

#![expect(
    unused_variables,
    dead_code,
    reason = "the scaffold: U2 fills this file"
)]

pub(super) mod limits;
mod tier;

use anyhow::Result;
use hamlet_messages::realm::budget::Payer;
use hamlet_messages::realm::{
    Found, FoundAnswer, FoundRefusal, HamletId, Tier,
};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::AgentNumber;

use super::act::{Act, Decision};
use super::callers::RealmWide;
use super::calls::Made;
use super::change::{self, Affected, Rows, Writer};
use super::{hire, steps, tasks, tree};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Hamlet {
    pub id: HamletId,
    pub mayor: AgentNumber,
    pub tier: Tier,
    pub limits: limits::Limits,
}

impl Hamlet {
    /// The most hires the hamlet holds (H6).
    pub fn most(&self) -> u32 {
        todo!()
    }
}

pub(super) async fn hamlet(
    rows: &Rows<'_>,
    id: &HamletId,
) -> Result<Option<Hamlet>> {
    todo!()
}

pub(super) async fn mayors(
    rows: &Rows<'_>,
) -> Result<Vec<AgentNumber>> {
    todo!()
}

/// A new hamlet's id, a random UUID.
pub(super) fn fresh() -> HamletId {
    todo!()
}

#[derive(Debug, PartialEq)]
pub(super) enum Change {
    /// The realm's row and its one chief, at the first start.
    First,
    /// A hamlet, with the chief as its chief and `mayor` as its
    /// mayor.
    Found {
        hamlet: HamletId,
        name: String,
        mayor: AgentNumber,
        tier: Tier,
    },
    Mayor {
        hamlet: HamletId,
        mayor: AgentNumber,
    },
    Tier {
        hamlet: HamletId,
        tier: Tier,
    },
    Limits(limits::Kept),
}

pub(super) async fn apply(
    writer: &Writer<'_>,
    change: Change,
) -> Result<Affected> {
    match change {
        Change::First => first(writer).await,
        Change::Found {
            hamlet,
            name,
            mayor,
            tier,
        } => found(writer, hamlet, name, mayor, tier).await,
        Change::Mayor { hamlet, mayor } => {
            set_mayor(writer, hamlet, mayor).await
        }
        Change::Tier { hamlet, tier } => {
            tier::set(writer, hamlet, tier).await
        }
        Change::Limits(kept) => limits::set(writer, kept).await,
    }
}

async fn first(writer: &Writer<'_>) -> Result<Affected> {
    todo!()
}

async fn found(
    writer: &Writer<'_>,
    hamlet: HamletId,
    name: String,
    mayor: AgentNumber,
    tier: Tier,
) -> Result<Affected> {
    todo!()
}

async fn set_mayor(
    writer: &Writer<'_>,
    hamlet: HamletId,
    mayor: AgentNumber,
) -> Result<Affected> {
    todo!()
}

pub(super) fn first_start() -> Vec<change::Change> {
    let hand = Hand::NUMBER;
    vec![
        Change::First.into(),
        tree::Change::Hand.into(),
        tasks::Change::Standing {
            agent: hand,
            payer: Payer::Realm,
        }
        .into(),
        steps::Change::Begin {
            row: hire::row(hand),
        }
        .into(),
    ]
}

pub(super) struct Founding {
    profile: Option<Option<tree::Profile>>,
    hamlet: HamletId,
    mayor: AgentNumber,
    blank: hamlet_messages::realm::ProfileId,
}

impl<By: RealmWide> Act<By> for Found {
    type Output = FoundAnswer<FoundRefusal>;
    type Loaded = Founding;
    /// The hamlet and its mayor.
    type Done = (HamletId, AgentNumber);

    async fn load(
        &self,
        by: &By,
        rows: &Rows<'_>,
    ) -> Result<Founding> {
        todo!()
    }

    fn decide(
        &self,
        by: &By,
        founding: Founding,
    ) -> Decision<Self::Done, Self::Output> {
        todo!()
    }

    fn answer(&self, done: Self::Done, made: &Made) -> Self::Output {
        todo!()
    }
}

/// The payer of an agent's own spending: its hamlet, or the realm
/// for the Hand.
pub(super) fn payer_of(lineage: &tree::Lineage) -> Payer {
    todo!()
}
