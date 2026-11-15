//! The chief's limits, a hamlet's and the realm's own: the `limits`
//! column of the `hamlet` and `realm` rows, and `SetLimits` (H22).

#![expect(
    unused_variables,
    dead_code,
    reason = "the scaffold: U2 fills this file"
)]

use anyhow::Result;
use hamlet_messages::realm::budget::{BudgetLimit, Payer};
use hamlet_messages::realm::{
    Done, HamletId, HamletRefusal, SetLimits,
};
use hamlet_messages::sender::Client;
use serde::{Deserialize, Serialize};

use super::super::act::{Act, Decision};
use super::super::calls::Made;
use super::super::change::{Affected, Rows, Writer};

#[derive(
    Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(default)]
pub(in crate::realm) struct Limits {
    budget: Budget,
    pub road: RoadRules,
}

impl Limits {
    /// A hamlet's limits from its row's `limits` column.
    pub(super) fn of_row(text: &str) -> Result<Self> {
        todo!()
    }
}

#[derive(
    Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(default)]
struct RealmLimits {
    budget: Budget,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(in crate::realm) struct Budget {
    /// In millionths of a dollar. `None` bounds nothing.
    pub micros: Option<u64>,
    /// Model time. `None` bounds nothing.
    pub ms: Option<u64>,
    pub period: Period,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            micros: Some(50_000_000),
            ms: None,
            period: Period::Day,
        }
    }
}

impl Budget {
    fn set(&mut self, limit: &BudgetLimit) {
        todo!()
    }
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub(in crate::realm) enum Period {
    Day,
    Month,
}

/// The price of a laid road and how many a hamlet holds (H3).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(in crate::realm) struct RoadRules {
    /// In millionths of a dollar.
    pub price: u64,
    pub most: u32,
    /// The days with no letter after which a laid road shows as
    /// grown over.
    pub grows_over_days: u32,
}

impl Default for RoadRules {
    fn default() -> Self {
        Self {
            price: 10_000,
            most: 24,
            grows_over_days: 7,
        }
    }
}

/// The `limits` column of a new hamlet's row: the defaults.
pub(super) fn new_hamlet() -> Result<String> {
    todo!()
}

/// The `limits` column of the realm's row at its first start: the
/// defaults.
pub(super) fn new_realm() -> Result<String> {
    todo!()
}

pub(in crate::realm) async fn budget(
    rows: &Rows<'_>,
    payer: &Payer,
) -> Result<Budget> {
    todo!()
}

async fn realm(rows: &Rows<'_>) -> Result<RealmLimits> {
    todo!()
}

/// Limits with the row that keeps them.
#[derive(Debug, PartialEq)]
pub(in crate::realm) struct Kept(Row);

#[derive(Debug, PartialEq)]
enum Row {
    Realm(RealmLimits),
    Hamlet(HamletId, Limits),
}

impl Kept {
    fn budget_mut(&mut self) -> &mut Budget {
        todo!()
    }
}

impl Act<Client> for SetLimits {
    type Output = Done<HamletRefusal>;
    /// `None` for a hamlet the realm does not hold.
    type Loaded = Option<Kept>;
    type Done = ();

    async fn load(
        &self,
        by: &Client,
        rows: &Rows<'_>,
    ) -> Result<Option<Kept>> {
        todo!()
    }

    fn decide(
        &self,
        by: &Client,
        kept: Option<Kept>,
    ) -> Decision<(), Self::Output> {
        todo!()
    }

    fn answer(&self, (): (), made: &Made) -> Self::Output {
        todo!()
    }
}

pub(super) async fn set(
    writer: &Writer<'_>,
    kept: Kept,
) -> Result<Affected> {
    todo!()
}
