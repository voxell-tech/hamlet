//! The realm's calls about spending (H8): what a payer spent in money
//! and in model time. Taking budgets out of the product deletes this
//! module.

use hamlet_actor::Action;
use serde::{Deserialize, Serialize};

use super::{ChiefRequest, Done, HamletId, HamletRefusal, Standing};
use crate::Message;
use crate::address::{Agent, Realm};
use crate::workspace::{CallId, RealmTask};

/// The hamlet or the realm a task's spending is charged to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Payer {
    Realm,
    Hamlet { hamlet: HamletId },
}

impl std::fmt::Display for Payer {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        match self {
            Self::Realm => f.write_str("the realm"),
            Self::Hamlet { hamlet } => {
                write!(f, "the hamlet `{}`", hamlet.0)
            }
        }
    }
}

/// One limit of a payer's budget, as a chief sets it (H8).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BudgetLimit {
    /// The most the payer spends in a period, in millionths of a
    /// dollar.
    Cost { micros: u64 },
    /// The most model time the payer spends in a period.
    Time { ms: u64 },
}

/// Whether the caller may make a model call for `task`. Made by an
/// agent before each model call. A read: it changes nothing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaySpend {
    pub task: RealmTask,
}

impl Action for MaySpend {
    type Output = Done<SpendRefusal>;
    const NAME: &'static str = "may_spend";
}

impl Message for MaySpend {
    type To = Realm;
    type From = Agent;
}

/// Why a model call may not be made: the caller's standing, or the
/// budget.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
#[serde(untagged)]
pub enum SpendRefusal {
    #[error("{0}")]
    Standing(Standing),
    #[error("{0}")]
    Budget(Budget),
}

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Budget {
    #[error("the realm holds no such task")]
    NoSuchTask,
    #[error(
        "the budget of {payer} has run out: {spent_micros} of \
         {budget_micros} millionths of a dollar spent; a chief \
         starts it again with /reset-budget"
    )]
    OverCost {
        payer: Payer,
        spent_micros: u64,
        budget_micros: u64,
    },
    #[error(
        "the model time of {payer} has run out: {spent_ms} of \
         {budget_ms} ms spent; a chief starts it again with \
         /reset-budget"
    )]
    OverTime {
        payer: Payer,
        spent_ms: u64,
        budget_ms: u64,
    },
}

/// Charges one model call to `task`'s payer. Made by an agent after
/// the model call. Bare: a repeat with the same `call` is counted
/// once, by the charge table's key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Charge {
    /// The id of the model call.
    pub call: CallId,
    pub task: RealmTask,
    /// Written by the agent, so a `u32`: at most $4,294 a call.
    pub micros: u32,
    /// How long the model call took. Written by the agent.
    pub took_ms: u32,
}

impl Action for Charge {
    type Output = ();
    const NAME: &'static str = "charge";
}

impl Message for Charge {
    type To = Realm;
    type From = Agent;
}

/// Starts a payer's period again. The chief's only.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResetBudget {
    pub payer: Payer,
}

impl ChiefRequest for ResetBudget {
    const NAME: &'static str = "chief_reset_budget";
    type Answer = Done<HamletRefusal>;
}
