//! The `decision` table: the record of what was decided, by whom
//! and why (H17).

#![expect(
    unused_variables,
    dead_code,
    reason = "the scaffold: U2 fills this file"
)]

use anyhow::Result;
use hamlet_messages::realm::{HamletId, Tier};
use hamlet_messages::workspace::AgentNumber;

use super::change::Writer;

/// Who decided.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum By {
    /// The realm's one chief.
    Chief,
    Agent(AgentNumber),
}

/// One entry of the record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Entry {
    Founded {
        hamlet: HamletId,
        by: By,
    },
    Hired {
        hire: AgentNumber,
        by: By,
        why: Option<String>,
    },
    Ended {
        hire: AgentNumber,
        by: By,
        why: Option<String>,
    },
    Seated {
        hire: AgentNumber,
        by: By,
    },
    Road {
        a: AgentNumber,
        b: AgentNumber,
        by: By,
    },
    Unroad {
        a: AgentNumber,
        b: AgentNumber,
        by: By,
    },
    Tools {
        hire: AgentNumber,
        by: By,
    },
    Tier {
        hamlet: HamletId,
        tier: Tier,
        by: By,
    },
    /// The limits of a hamlet, or of the realm when `hamlet` is
    /// `None`.
    Limits {
        hamlet: Option<HamletId>,
        by: By,
    },
}

/// Writes `entries`. Only [`super::change::Transaction::commit`]
/// calls it.
pub(super) async fn write(
    writer: &Writer<'_>,
    entries: Vec<Entry>,
) -> Result<()> {
    todo!()
}
