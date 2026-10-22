//! The realm as the server shows it: the snapshot a connection begins
//! with and the changes that follow.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::client::{
    Limits, Period, Permission, ProviderView, SettingsView, Wait,
};

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct State {
    pub agents: Vec<Agent>,
    pub chief: String,
    pub closed: Vec<ClosedHamlet>,
    pub deeds: Vec<Deed>,
    pub epoch: i64,
    pub grants: Vec<Grant>,
    pub hamlets: Vec<Hamlet>,
    pub houses: String,
    pub library: Vec<LibrarySkill>,
    pub providers: Vec<ProviderView>,
    pub revision: i64,
    pub sandbox: String,
    pub settings: SettingsView,
    pub starters: Vec<Skill>,
    pub waits: Vec<Wait>,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Changes {
    pub changes: Vec<Update>,
    pub epoch: i64,
    pub revision: i64,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(tag = "type", deny_unknown_fields)]
#[expect(
    clippy::large_enum_variant,
    reason = "a wire type; a box would show in every pattern"
)]
pub enum Update {
    #[serde(rename = "hamlet")]
    Hamlet { hamlet: Hamlet },
    #[serde(rename = "agent")]
    Agent { agent: Agent },
    #[serde(rename = "agent-gone")]
    AgentGone {
        #[serde(rename = "ref")]
        ref_: AgentRef,
    },
    #[serde(rename = "wait")]
    Wait { wait: Wait },
    #[serde(rename = "wait-closed")]
    WaitClosed { how: String, id: String },
    #[serde(rename = "grants")]
    Grants { grants: Vec<Grant> },
    #[serde(rename = "settings")]
    Settings { settings: SettingsView },
    #[serde(rename = "providers")]
    Providers { providers: Vec<ProviderView> },
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Address {
    pub agent: String,
    pub hamlet: String,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct AgentRef {
    pub hamlet: String,
    pub number: u64,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Agent {
    pub activity: AgentActivity,
    pub busy: bool,
    pub consulting: bool,
    pub conversations: Vec<ConversationView>,
    pub faulted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "Left")]
    pub left: Option<Left>,
    pub mayor: bool,
    pub model: String,
    pub name: String,
    pub purpose: String,
    #[serde(rename = "ref")]
    pub ref_: AgentRef,
    pub saying: AgentSaying,
    pub since: u64,
    pub skills: Vec<String>,
    pub spent: AgentSpent,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct AgentActivity {
    pub doing: Doing,
    #[serde(rename = "doingWords")]
    pub doing_words: String,
    pub failures: u64,
    pub step: u64,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct AgentSaying {
    pub text: String,
    pub thought: bool,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct AgentSpent {
    pub cost: String,
    pub micros: i64,
    pub ms: i64,
    pub tokens: i64,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Amount {
    pub micros: i64,
    pub ms: i64,
    pub tokens: i64,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub enum Doing {
    #[serde(rename = "rest")]
    Rest,
    #[serde(rename = "think")]
    Think,
    #[serde(rename = "read-file")]
    ReadFile,
    #[serde(rename = "read-notes")]
    ReadNotes,
    #[serde(rename = "read-skill")]
    ReadSkill,
    #[serde(rename = "write-file")]
    WriteFile,
    #[serde(rename = "write-notes")]
    WriteNotes,
    #[serde(rename = "write-skill")]
    WriteSkill,
    #[serde(rename = "run")]
    Run,
    #[serde(rename = "fetch")]
    Fetch,
    #[serde(rename = "custom")]
    Custom,
    #[serde(rename = "await")]
    Await,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct ConversationView {
    pub count: i64,
    pub kind: ConversationViewKind,
    #[serde(deserialize_with = "Option::deserialize")]
    pub last: Option<i64>,
    pub with: AgentRef,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub enum ConversationViewKind {
    #[serde(rename = "letters")]
    Letters,
    #[serde(rename = "questions")]
    Questions,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Left {
    pub at: u64,
    pub by: String,
    pub how: LeftHow,
    #[serde(deserialize_with = "Option::deserialize")]
    pub into: Option<AgentRef>,
    pub reason: String,
    pub words: String,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub enum LeftHow {
    #[serde(rename = "removed")]
    Removed,
    #[serde(rename = "merged")]
    Merged,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Hamlet {
    #[serde(deserialize_with = "Option::deserialize")]
    pub at: Option<TownAt>,
    pub before: Vec<String>,
    pub budget: Report,
    #[serde(deserialize_with = "Option::deserialize")]
    pub folder: Option<String>,
    pub id: String,
    pub limits: Limits,
    pub mode: HamletMode,
    #[serde(rename = "modeWords")]
    pub mode_words: String,
    pub name: String,
    pub roads: Vec<Road>,
    pub votes: Vec<VoteView>,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub enum HamletMode {
    #[serde(rename = "manual")]
    Manual,
    #[serde(rename = "auto")]
    Auto,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct ClosedHamlet {
    pub at: i64,
    pub by: String,
    pub id: String,
    pub name: String,
    pub reason: String,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct TownAt {
    pub x: f64,
    pub z: f64,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Road {
    pub a: AgentRef,
    pub b: AgentRef,
    #[serde(rename = "lastFrom")]
    #[serde(deserialize_with = "Option::deserialize")]
    pub last_from: Option<AgentRef>,
    pub level: RoadLevel,
    pub messages: u64,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub enum RoadLevel {
    #[serde(rename = "overgrown")]
    Overgrown,
    #[serde(rename = "dirt")]
    Dirt,
    #[serde(rename = "paved")]
    Paved,
    #[serde(rename = "rail")]
    Rail,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct VoteView {
    pub by: String,
    pub id: i64,
    pub motion: String,
    pub open: bool,
    pub overturnable: bool,
    pub report: String,
    pub voters: Vec<Voter>,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Voter {
    pub agent: AgentRef,
    #[serde(deserialize_with = "Option::deserialize")]
    pub cast: Option<VoterCast>,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub enum VoterCast {
    #[serde(rename = "yes")]
    Yes,
    #[serde(rename = "no")]
    No,
    #[serde(rename = "abstain")]
    Abstain,
    #[serde(rename = "none")]
    None,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    #[serde(deserialize_with = "Option::deserialize")]
    pub agent: Option<AgentRef>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub by: Option<String>,
    pub name: String,
    pub permission: Permission,
    pub source: GrantSource,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub enum GrantSource {
    #[serde(rename = "limits")]
    Limits,
    #[serde(rename = "grant")]
    Grant,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Deed {
    pub description: String,
    pub name: String,
    pub usage: String,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Skill {
    pub description: String,
    pub name: String,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct LibrarySkill {
    pub by: String,
    pub description: String,
    pub name: String,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub caps: ReportCaps,
    pub gone: f64,
    #[serde(deserialize_with = "Option::deserialize")]
    pub left: Option<f64>,
    pub lines: Vec<String>,
    pub period: Period,
    #[serde(rename = "periodWords")]
    pub period_words: String,
    #[serde(deserialize_with = "Option::deserialize")]
    pub remains: Option<String>,
    pub spent: Amount,
    pub summary: String,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct ReportCaps {
    #[serde(deserialize_with = "Option::deserialize")]
    pub micros: Option<i64>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub ms: Option<i64>,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct UsageView {
    pub cached: u64,
    pub calls: u64,
    pub input: u64,
    #[serde(rename = "lastInput")]
    pub last_input: u64,
    pub micros: u64,
    pub ms: u64,
    pub output: u64,
    pub reasoning: u64,
    pub summary: String,
    pub unreported: u64,
}
