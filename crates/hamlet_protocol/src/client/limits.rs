//! The chief's limits on the hamlets and their agents.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub asking: LimitsAsking,
    pub budget: LimitsBudget,
    pub efforts: std::collections::BTreeMap<String, crate::Effort>,
    pub hamlets: HamletLimits,
    pub leaving: LimitsLeaving,
    pub mayor: LimitsMayor,
    #[serde(deserialize_with = "Option::deserialize")]
    pub model: Option<String>,
    pub models: Vec<String>,
    #[serde(rename = "mostAgents")]
    pub most_agents: i64,
    #[serde(rename = "mostRoads")]
    pub most_roads: i64,
    pub permissions: LimitsPermissions,
    pub pinned: std::collections::BTreeMap<String, String>,
    pub road: LimitsRoad,
    pub task: LimitsTask,
    pub votes: VoteLimits,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct LimitsAsking {
    #[serde(rename = "mayorReview")]
    pub mayor_review: LimitsAskingMayorReview,
    pub questions: i64,
    pub refusals: i64,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub enum LimitsAskingMayorReview {
    #[serde(rename = "always")]
    Always,
    #[serde(rename = "never")]
    Never,
    #[serde(rename = "remote")]
    Remote,
}

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
    #[serde(deserialize_with = "Option::deserialize")]
    pub tokens: Option<i64>,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct LimitsLeaving {
    #[serde(rename = "mergedNotes")]
    pub merged_notes: i64,
    #[serde(rename = "protectedMinutes")]
    pub protected_minutes: i64,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct LimitsMayor {
    #[serde(rename = "buildRoads")]
    pub build_roads: bool,
    #[serde(rename = "createAgents")]
    pub create_agents: bool,
    #[serde(rename = "mergeAgents")]
    pub merge_agents: bool,
    #[serde(rename = "removeAgents")]
    pub remove_agents: bool,
    #[serde(rename = "toldDays")]
    pub told_days: i64,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct LimitsPermissions {
    pub network: Vec<String>,
    pub unconfined: Vec<String>,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct LimitsRoad {
    pub cost: f64,
    #[serde(rename = "growsOverDays")]
    pub grows_over_days: i64,
    pub minutes: i64,
    pub tokens: i64,
}

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
    #[serde(deserialize_with = "Option::deserialize")]
    pub tokens: Option<i64>,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct VoteLimits {
    #[serde(rename = "ballotMinutes")]
    pub ballot_minutes: i64,
    #[serde(rename = "groundedOnly")]
    pub grounded_only: bool,
    #[serde(rename = "leastVoters")]
    pub least_voters: i64,
    #[serde(rename = "openMinutes")]
    pub open_minutes: i64,
    #[serde(rename = "passOf")]
    pub pass_of: i64,
    #[serde(rename = "passYes")]
    pub pass_yes: i64,
    #[serde(rename = "retryMinutes")]
    pub retry_minutes: i64,
    pub sealed: bool,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct HamletLimits {
    pub most: i64,
    #[serde(rename = "removalMayors")]
    pub removal_mayors: i64,
    #[serde(rename = "roadsBetween")]
    pub roads_between: bool,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub enum Period {
    #[serde(rename = "day")]
    Day,
    #[serde(rename = "month")]
    Month,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub enum Permission {
    #[serde(rename = "network")]
    Network,
    #[serde(rename = "unconfined")]
    Unconfined,
}
