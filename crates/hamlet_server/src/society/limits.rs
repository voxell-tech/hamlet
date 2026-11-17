//! The chief's limits: each hamlet's, and the realm's own (H22).

use anyhow::{Context, Result};
use diesel::prelude::*;
use hamlet_store::Db;
use hamlet_store::ids::{ChiefId, HamletId};
use hamlet_store::schema::{document, hamlet};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::refusal::Refusal;
use super::spend::micros;
use super::{authority, hamlets};
use crate::realm::change::{Change, Done, refuse};

/// Millionths of a dollar.
pub(crate) type Micros = i64;

/// A hamlet's limits. Every check reads them as they are now.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct Limits {
    pub budget: Budget,
    pub road: RoadRules,
    pub reserved: Reserved,
    /// The hamlet's own model, else the realm's.
    pub model: Option<String>,
    /// How many days of decisions each hire is told (H17).
    pub told_days: i64,
    pub asking: ChiefQuestions,
    /// How long after a chief's task a hire may not be ended or
    /// merged by a lead (H7).
    pub protected_minutes: i64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            budget: Budget::default(),
            road: RoadRules::default(),
            reserved: Reserved::default(),
            model: None,
            told_days: 7,
            asking: ChiefQuestions::default(),
            protected_minutes: 0,
        }
    }
}

impl Limits {
    /// What is wrong with these limits, in words; none when they
    /// hold.
    pub fn fault(&self) -> Option<String> {
        let negative = [
            ("a road's price", self.road.price),
            ("the most roads", self.road.most),
            ("the days a road stays", self.road.grows_over_days),
            ("the days of decisions told", self.told_days),
            ("the questions a turn puts", self.asking.questions),
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

/// What a new hamlet, and the Hand, may spend in a period: $50.
pub(crate) const DEFAULT_BUDGET: Micros = 50_000_000;

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
            cost: Some(DEFAULT_BUDGET),
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

/// How much one turn may bring to a chief: its questions, its
/// refused requests and its review (H10, H11).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct ChiefQuestions {
    pub questions: i64,
    pub refusals: i64,
    pub review: Review,
}

impl Default for ChiefQuestions {
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

/// One limit with its new value.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Limit {
    BudgetCost(Option<Micros>),
    BudgetMinutes(Option<i64>),
    BudgetPeriod(Period),
    RoadPrice(Micros),
    RoadMost(i64),
    RoadGrowsOverDays(i64),
    ReservedHire(bool),
    ReservedRoads(bool),
    ReservedEnd(bool),
    ReservedMerge(bool),
    AskingQuestions(i64),
    AskingRefusals(i64),
    AskingReview(Review),
    Model(Option<String>),
    ToldDays(i64),
    ProtectedMinutes(i64),
    RoadsBetween(bool),
}

impl Limit {
    /// The limit `key` of `section`, empty for a key outside every
    /// section, set to `value`, as the Settings name it.
    pub fn read(
        section: &str,
        key: &str,
        value: Value,
    ) -> Result<Self, Refusal> {
        fn of<T: DeserializeOwned>(
            value: Value,
        ) -> Result<T, Refusal> {
            serde_json::from_value(value)
                .map_err(|_| Refusal::LimitsUnfit)
        }
        Ok(match (section, key) {
            ("budget", "cost") => Self::BudgetCost(
                of::<Option<f64>>(value)?.map(micros),
            ),
            ("budget", "minutes") => Self::BudgetMinutes(of(value)?),
            ("budget", "period") => Self::BudgetPeriod(of(value)?),
            ("road", "price") => {
                Self::RoadPrice(micros(of::<f64>(value)?))
            }
            ("road", "most") => Self::RoadMost(of(value)?),
            ("road", "growsOverDays") => {
                Self::RoadGrowsOverDays(of(value)?)
            }
            ("reserved", "hire") => Self::ReservedHire(of(value)?),
            ("reserved", "roads") => Self::ReservedRoads(of(value)?),
            ("reserved", "end") => Self::ReservedEnd(of(value)?),
            ("reserved", "merge") => Self::ReservedMerge(of(value)?),
            ("asking", "questions") => {
                Self::AskingQuestions(of(value)?)
            }
            ("asking", "refusals") => {
                Self::AskingRefusals(of(value)?)
            }
            ("asking", "review") => Self::AskingReview(of(value)?),
            ("", "model") => Self::Model(of(value)?),
            ("", "toldDays") => Self::ToldDays(of(value)?),
            ("", "protectedMinutes") => {
                Self::ProtectedMinutes(of(value)?)
            }
            ("", "roadsBetween") => Self::RoadsBetween(of(value)?),
            _ => return Err(Refusal::LimitsUnfit),
        })
    }

    /// Sets this limit in `budget`, or gives it back when it is not
    /// a budget's.
    fn set_budget(self, budget: &mut Budget) -> Result<(), Self> {
        match self {
            Self::BudgetCost(cost) => budget.cost = cost,
            Self::BudgetMinutes(minutes) => budget.minutes = minutes,
            Self::BudgetPeriod(period) => budget.period = period,
            other => return Err(other),
        }
        Ok(())
    }
}

impl Limits {
    /// Sets `limit`, refused when it is not a hamlet's.
    pub fn set(&mut self, limit: Limit) -> Result<(), Refusal> {
        let Err(limit) = limit.set_budget(&mut self.budget) else {
            return Ok(());
        };
        match limit {
            Limit::RoadPrice(price) => self.road.price = price,
            Limit::RoadMost(most) => self.road.most = most,
            Limit::RoadGrowsOverDays(days) => {
                self.road.grows_over_days = days;
            }
            Limit::ReservedHire(on) => self.reserved.hire = on,
            Limit::ReservedRoads(on) => self.reserved.roads = on,
            Limit::ReservedEnd(on) => self.reserved.end = on,
            Limit::ReservedMerge(on) => self.reserved.merge = on,
            Limit::AskingQuestions(most) => {
                self.asking.questions = most;
            }
            Limit::AskingRefusals(most) => {
                self.asking.refusals = most;
            }
            Limit::AskingReview(review) => {
                self.asking.review = review;
            }
            Limit::Model(model) => self.model = model,
            Limit::ToldDays(days) => self.told_days = days,
            Limit::ProtectedMinutes(minutes) => {
                self.protected_minutes = minutes;
            }
            Limit::BudgetCost(_)
            | Limit::BudgetMinutes(_)
            | Limit::BudgetPeriod(_)
            | Limit::RoadsBetween(_) => {
                return Err(Refusal::LimitsUnfit);
            }
        }
        Ok(())
    }
}

impl RealmLimits {
    /// Sets `limit`, refused when it is not the realm's.
    pub fn set(&mut self, limit: Limit) -> Result<(), Refusal> {
        match limit.set_budget(&mut self.budget) {
            Ok(()) => {}
            Err(Limit::RoadsBetween(on)) => self.roads_between = on,
            Err(_) => return Err(Refusal::LimitsUnfit),
        }
        Ok(())
    }
}

/// Sets `limit` of the hamlet `hamlet`, or of the realm's limits
/// when none; `chief` is who the realm's limits are kept by.
pub(crate) fn set_one(
    cx: &mut Change<'_>,
    chief: ChiefId,
    hamlet: Option<&HamletId>,
    limit: Limit,
) -> Done<()> {
    match hamlet {
        Some(id) => set_hamlet(cx, id, |own| own.set(limit)),
        None => set_realm(cx, chief, |own| own.set(limit)),
    }
}

/// Replaces the limits of the hamlet `id` with what `edit` makes of
/// them.
pub(crate) fn set_hamlet(
    cx: &mut Change<'_>,
    id: &HamletId,
    edit: impl FnOnce(&mut Limits) -> Result<(), Refusal>,
) -> Done<()> {
    if hamlets::find(cx.db, id)?.is_none() {
        refuse!(Refusal::NoHamlet { id: id.to_string() });
    }
    if !authority::may_govern(cx.db, cx.actor, id)? {
        refuse!(Refusal::NotChief {
            hamlet: id.to_string()
        });
    }
    let mut own = of(cx.db, id)?;
    if let Err(refusal) = edit(&mut own) {
        refuse!(refusal);
    }
    if let Some(why) = own.fault() {
        refuse!(Refusal::BadLimits { why });
    }
    hamlets::set_limits(cx, id, &own)
}

/// Replaces the realm's limits with what `edit` makes of them, kept
/// by `chief`.
pub(crate) fn set_realm(
    cx: &mut Change<'_>,
    chief: ChiefId,
    edit: impl FnOnce(&mut RealmLimits) -> Result<(), Refusal>,
) -> Done<()> {
    if !authority::may_rule(cx.db, cx.actor)? {
        refuse!(Refusal::NotRealmChief);
    }
    let mut own = realm(cx.db)?;
    if let Err(refusal) = edit(&mut own) {
        refuse!(refusal);
    }
    if let Some(why) = own.budget.fault() {
        refuse!(Refusal::BadLimits { why });
    }
    diesel::replace_into(document::table)
        .values((
            document::kind.eq("limits"),
            document::value.eq(serde_json::to_string(&own)?),
            document::at.eq(cx.at),
            document::chief.eq(chief),
        ))
        .execute(cx.db)?;
    Ok(Ok(()))
}

/// What setting the limits of `hamlet`, or the realm's, comes to.
pub(crate) fn saved(hamlet: Option<&HamletId>) -> String {
    match hamlet {
        Some(id) => format!("The limits of `{id}` are saved."),
        None => "The realm's limits are saved.".into(),
    }
}
