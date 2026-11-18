//! The realm's calls about spending (H8).

use hamlet_actor::Action;
use hamlet_protocol::client::RequestId;
use serde::{Deserialize, Serialize};

use super::oversight::Hold;
use super::world::Spend;
use super::{
    AgentRefusal, ChiefRequest, Done, HamletId, HamletRefusal,
    Refusable, SocietyTool, Standing, ToolCall,
};
use crate::address::{Agent, Realm};
use crate::workspace::{AgentNumber, CallId, RealmTask};
use crate::{Message, worded};

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
    Cost {
        micros: u64,
    },
    /// The most model time the payer spends in a period.
    Time {
        ms: u64,
    },
    Period {
        period: Period,
    },
}

/// The time a budget's spending is counted over, from its start.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Period {
    Day,
    Month,
}

/// Whether the caller may make a model call for `task`. A read: it
/// changes nothing.
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

/// Why a model call may not be made.
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
    #[error("{0}")]
    Held(Hold),
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
    #[error(
        "`{name}`'s allowance of {} is spent. Its lead or a chief can \
         raise it.",
        money(*allowance_micros)
    )]
    OverAllowance {
        hire: AgentNumber,
        /// The hire's name, as in `hire#3 (Mira)`.
        name: String,
        spent_micros: u64,
        allowance_micros: u64,
    },
}

/// `micros` in dollars to the cent, as `$1.23`, or `<$0.01` for less
/// than a cent and more than nothing.
fn money(micros: u64) -> String {
    if (1..10_000).contains(&micros) {
        return "<$0.01".to_owned();
    }
    let cents = (u128::from(micros) + 5_000) / 10_000;
    format!("${}.{:02}", cents / 100, cents % 100)
}

/// Charges one model call to `task`'s payer. A repeat with the same
/// `call` is counted once.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Charge {
    /// The id of the model call.
    pub call: CallId,
    pub task: RealmTask,
    pub micros: u32,
    pub took_ms: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<u32>,
}

impl Action for Charge {
    type Output = ();
    const NAME: &'static str = "charge";
}

impl Message for Charge {
    type To = Realm;
    type From = Agent;
}

/// Starts a payer's period again.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResetBudget {
    pub payer: Payer,
}

impl ChiefRequest for ResetBudget {
    const NAME: &'static str = "chief_reset_budget";
    type Answer = Done<HamletRefusal>;
}

/// Opens a task of the caller's own for a turn no line or letter
/// started (H12). A repeat carries the same `request`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenTask {
    pub request: RequestId,
    pub turn: TaskTurn,
}

/// The turn a task is opened for.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum TaskTurn {
    /// A chief's `/retry`.
    Retry,
    Reflection,
}

impl Action for OpenTask {
    type Output = TaskOpened;
    const NAME: &'static str = "open_task";
}

impl Message for OpenTask {
    type To = Realm;
    type From = Agent;
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TaskOpened {
    Opened {
        task: RealmTask,
    },
    #[serde(serialize_with = "worded")]
    Refused {
        why: Standing,
    },
}

/// Reads what the caller may spend: what its payer spent this period
/// against its budget, what the caller spent, and its allowance.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadBudget {
    /// The task whose payer is read; absent for the caller's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<RealmTask>,
}

impl ToolCall for ReadBudget {
    const TOOL: SocietyTool = SocietyTool::ReadBudget;
    type Answer = BudgetRead<AgentRefusal<ReadBudgetRefusal>>;
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    bound(
        serialize = "W: Serialize + std::fmt::Display",
        deserialize = "W: Deserialize<'de>"
    )
)]
pub enum BudgetRead<W> {
    Budget {
        spend: Spend,
        period: Period,
        /// What the caller itself spent this period.
        own_micros: u64,
        /// The caller's allowance; absent when it has none.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        allowance_micros: Option<u64>,
    },
    #[serde(serialize_with = "worded")]
    Refused { why: W },
}

impl<W> Refusable for BudgetRead<W> {
    type Why = W;
    fn refused(why: W) -> Self {
        Self::Refused { why }
    }
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
pub enum ReadBudgetRefusal {
    #[error("the realm holds no such task")]
    NoSuchTask,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_allowance_is_shown_in_dollars_to_the_cent() {
        let spent = |allowance_micros| {
            Budget::OverAllowance {
                hire: AgentNumber(3),
                name: "hire#3".to_owned(),
                spent_micros: allowance_micros,
                allowance_micros,
            }
            .to_string()
        };

        assert_eq!(
            [spent(1_235_000), spent(9_999), spent(0)],
            [
                "`hire#3`'s allowance of $1.24 is spent. Its lead \
                 or a chief can raise it.",
                "`hire#3`'s allowance of <$0.01 is spent. Its lead \
                 or a chief can raise it.",
                "`hire#3`'s allowance of $0.00 is spent. Its lead \
                 or a chief can raise it.",
            ]
        );
    }
}
