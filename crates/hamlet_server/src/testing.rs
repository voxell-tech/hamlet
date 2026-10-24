//! Helpers shared by the crate's tests.

use anyhow::{Result, ensure};
use rusqlite::Connection;

use crate::harness::{Config, Pi};
use crate::{Outcome, Overrides, limits, record};

/// A harness configuration whose pi never starts, for a test whose
/// home binds no conversation.
pub(crate) fn config(overrides: Overrides) -> Config {
    Config {
        pi: Pi {
            node: "node".into(),
            entry: "unused".into(),
            faux: None,
        },
        tools: Vec::new(),
        overrides,
    }
}

/// Sets the limits of `hamlet` as its first chief would.
pub(crate) fn set_limits(
    db: &Connection,
    hamlet: &str,
    limits: limits::Limits,
) -> Result<()> {
    let record =
        record::read(db, hamlet)?.expect("a hamlet to limit");
    let operation = record::RecordOperation {
        hamlet: hamlet.into(),
        actor: record::Actor::Chief(record.chiefs[0].clone()),
        at: 0,
        reason: String::new(),
        task: None,
        change: record::Change::SetLimits {
            limits: record::Limits {
                chief: Box::new(limits),
                road_budget: record.limits.road_budget,
            },
            runs: Vec::new(),
        },
    };
    let outcome = crate::operation::change(db, &operation)?;
    ensure!(
        matches!(outcome, Outcome::Done(_)),
        "the limits were refused"
    );
    Ok(())
}
