//! What a chief's edits of memory and limits carry, read into the
//! terms the server keeps them in.

use anyhow::Result;
use hamlet_protocol::client::{self as protocol};

use crate::society::limits::{self, Limits};
use crate::society::refusal::Refusal;
use crate::spend::micros;

/// Refuses a memory path that is not a plain relative path under
/// `memory/`.
pub(crate) fn memory_path(path: &str) -> Result<String, Refusal> {
    let path = path.trim().trim_start_matches("memory/");
    let plain = !path.is_empty()
        && !path.starts_with('/')
        && path.split('/').all(|part| {
            !part.is_empty() && part != "." && part != ".."
        });
    if plain {
        Ok(format!("memory/{path}"))
    } else {
        Err(Refusal::NoFile { path: path.into() })
    }
}

pub(crate) fn budget_of(
    budget: &protocol::LimitsBudget,
) -> limits::Budget {
    limits::Budget {
        cost: budget.cost.map(micros),
        minutes: budget.minutes,
        period: match budget.period {
            protocol::Period::Day => limits::Period::Day,
            protocol::Period::Month => limits::Period::Month,
        },
    }
}

/// A hamlet's limits from their client form.
pub(crate) fn limits_of(shown: &protocol::Limits) -> Limits {
    Limits {
        budget: budget_of(&shown.budget),
        road: limits::RoadRules {
            price: micros(shown.road.price),
            most: shown.road.most,
            grows_over_days: shown.road.grows_over_days,
        },
        reserved: limits::Reserved {
            hire: shown.reserved.hire,
            roads: shown.reserved.roads,
            end: shown.reserved.end,
            merge: shown.reserved.merge,
        },
        model: shown.model.clone(),
        told_days: shown.told_days,
        asking: limits::Asking {
            questions: shown.asking.questions,
            refusals: shown.asking.refusals,
            review: match shown.asking.review {
                protocol::LimitsReview::Always => {
                    limits::Review::Always
                }
                protocol::LimitsReview::Remote => {
                    limits::Review::Remote
                }
                protocol::LimitsReview::Never => {
                    limits::Review::Never
                }
            },
        },
        protected_minutes: shown.protected_minutes,
    }
}
