//! The chief's limits: each hamlet's and the realm's own (H22).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A hamlet's limits.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub asking: LimitsAsking,
    pub budget: LimitsBudget,
    /// How deep its tree may be under its mayor.
    pub depth: i64,
    /// The hamlet's own model, else the realm's.
    #[serde(deserialize_with = "Option::deserialize")]
    pub model: Option<String>,
    /// The models its leads may choose; empty is any.
    pub models: Vec<String>,
    /// How long after a chief's task a lead may not end or merge the
    /// hire (H7).
    #[serde(rename = "protectedMinutes")]
    pub protected_minutes: i64,
    /// The leads' acts that wait for a chief.
    pub reserved: LimitsReserved,
    pub road: LimitsRoad,
    pub task: LimitsTask,
    /// How many days of decisions each hire is told (H17).
    #[serde(rename = "toldDays")]
    pub told_days: i64,
}

/// The realm's own limits.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct RealmLimits {
    /// The Hand's budget, and that of the workers started outside a
    /// hamlet.
    pub budget: LimitsBudget,
    /// Whether mayors may agree roads between their hamlets.
    #[serde(rename = "roadsBetween")]
    pub roads_between: bool,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct LimitsAsking {
    pub questions: i64,
    pub refusals: i64,
    pub review: LimitsReview,
}

/// When a lead reviews a chief's instruction to a hire under it
/// (H10).
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    JsonSchema,
)]
pub enum LimitsReview {
    #[serde(rename = "always")]
    Always,
    /// When the lead's model is served from another machine.
    #[serde(rename = "remote")]
    Remote,
    #[serde(rename = "never")]
    Never,
}

/// A budget for a period, in dollars and minutes of model time (H8).
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct LimitsBudget {
    #[serde(deserialize_with = "Option::deserialize")]
    pub cost: Option<f64>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub minutes: Option<i64>,
    pub period: Period,
}

/// The leads' acts a chief keeps for itself, so they wait for a
/// chief.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct LimitsReserved {
    pub end: bool,
    pub hire: bool,
    pub merge: bool,
    pub roads: bool,
}

/// What roads cost and how many a hamlet holds (H3).
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct LimitsRoad {
    #[serde(rename = "growsOverDays")]
    pub grows_over_days: i64,
    pub most: i64,
    /// Dollars.
    pub price: f64,
}

/// The most one task may do (H12).
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct LimitsTask {
    #[serde(deserialize_with = "Option::deserialize")]
    pub cost: Option<f64>,
    pub messages: i64,
    #[serde(deserialize_with = "Option::deserialize")]
    pub minutes: Option<i64>,
}

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    JsonSchema,
)]
pub enum Period {
    #[serde(rename = "day")]
    Day,
    #[serde(rename = "month")]
    Month,
}

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    JsonSchema,
)]
pub enum Permission {
    #[serde(rename = "network")]
    Network,
    #[serde(rename = "unconfined")]
    Unconfined,
}
