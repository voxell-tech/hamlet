//! The chief's limits: each hamlet's, and the realm's own (H22).

use anyhow::{Context, Result};
use diesel::prelude::*;
use hamlet_store::Db;
use hamlet_store::ids::HamletId;
use hamlet_store::schema::{document, hamlet};
use serde::{Deserialize, Serialize};

/// Millionths of a dollar.
pub(crate) type Micros = i64;

/// A hamlet's limits. Every check reads them as they are now.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct Limits {
    pub budget: Budget,
    pub task: TaskCaps,
    pub road: RoadRules,
    pub reserved: Reserved,
    /// The hamlet's own model, else the realm's.
    pub model: Option<String>,
    /// How many days of decisions each hire is told (H17).
    pub told_days: i64,
    pub asking: Asking,
    /// How long after a chief's task a hire may not be ended or
    /// merged by a lead (H7).
    pub protected_minutes: i64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            budget: Budget::default(),
            task: TaskCaps::default(),
            road: RoadRules::default(),
            reserved: Reserved::default(),
            model: None,
            told_days: 7,
            asking: Asking::default(),
            protected_minutes: 0,
        }
    }
}

impl Limits {
    /// What is wrong with these limits, in words; none when they
    /// hold.
    pub fn fault(&self) -> Option<String> {
        let negative = [
            ("a task's cost", self.task.cost.unwrap_or(0)),
            ("a task's minutes", self.task.minutes.unwrap_or(0)),
            ("a task's messages", self.task.messages),
            ("a road's price", self.road.price),
            ("the most roads", self.road.most),
            ("the days a road stays", self.road.grows_over_days),
            ("the days of decisions told", self.told_days),
            ("the questions a turn asks", self.asking.questions),
            ("the refusals a turn takes", self.asking.refusals),
            ("the protected minutes", self.protected_minutes),
        ]
        .into_iter()
        .find(|(_, value)| *value < 0);
        if let Some((what, _)) = negative {
            return Some(format!("{what} cannot be less than zero"));
        }
        self.budget.fault()
    }
}

impl Budget {
    /// What is wrong with this budget, in words; none when it holds.
    pub fn fault(&self) -> Option<String> {
        if self.cost.is_some_and(|cost| cost < 0)
            || self.minutes.is_some_and(|minutes| minutes < 0)
        {
            return Some("a budget cannot be less than zero".into());
        }
        None
    }
}

/// A budget for a period, on cost and model time (H8).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct Budget {
    pub cost: Option<Micros>,
    pub minutes: Option<i64>,
    pub period: Period,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            cost: Some(5_000_000),
            minutes: None,
            period: Period::Day,
        }
    }
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Period {
    Day,
    Month,
}

/// The most one task may do (H12).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct TaskCaps {
    pub cost: Option<Micros>,
    pub minutes: Option<i64>,
    pub messages: i64,
}

impl Default for TaskCaps {
    fn default() -> Self {
        Self {
            cost: Some(1_000_000),
            minutes: None,
            messages: 12,
        }
    }
}

/// What roads cost and how many a hamlet holds (H3).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct RoadRules {
    pub price: Micros,
    pub most: i64,
    pub grows_over_days: i64,
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

/// The leads' acts a chief keeps for itself, so they wait for a chief
/// (H22).
#[derive(
    Clone, Debug, Default, PartialEq, Serialize, Deserialize,
)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct Reserved {
    pub hire: bool,
    pub roads: bool,
    pub end: bool,
    pub merge: bool,
}

/// What may ask a chief in one turn (H10, H11).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct Asking {
    pub questions: i64,
    pub refusals: i64,
    pub review: Review,
}

impl Default for Asking {
    fn default() -> Self {
        Self {
            questions: 3,
            refusals: 3,
            review: Review::Remote,
        }
    }
}

/// When a lead reviews a chief's instruction to a hire under it
/// (H10).
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Review {
    Always,
    /// When the lead's model is served from another machine.
    Remote,
    Never,
}

/// The realm's own limits.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct RealmLimits {
    /// Whether mayors may agree roads between their hamlets.
    pub roads_between: bool,
    /// The budget of the Hand and of the workers it and the chiefs
    /// start outside a hamlet.
    pub budget: Budget,
}

impl Default for RealmLimits {
    fn default() -> Self {
        Self {
            roads_between: true,
            budget: Budget::default(),
        }
    }
}

/// The limits of `id`.
pub(crate) fn of(db: &mut Db, id: &HamletId) -> Result<Limits> {
    let text = hamlet::table
        .find(id)
        .select(hamlet::limits)
        .first::<String>(db)
        .with_context(|| format!("the limits of {id}"))?;
    Ok(serde_json::from_str(&text)?)
}

pub(crate) fn realm(db: &mut Db) -> Result<RealmLimits> {
    let text = document::table
        .find("limits")
        .select(document::value)
        .first::<String>(db)
        .optional()?;
    Ok(match text {
        Some(text) => serde_json::from_str(&text)?,
        None => RealmLimits::default(),
    })
}
