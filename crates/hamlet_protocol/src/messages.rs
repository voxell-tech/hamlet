#![allow(clippy::large_enum_variant)]

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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
pub struct Agent {
    pub activity: AgentActivity,
    pub busy: bool,
    pub consulting: bool,
    pub conversations: Vec<ConversationView>,
    pub faulted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
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
pub struct AgentRef {
    pub hamlet: String,
    pub number: u64,
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
#[serde(deny_unknown_fields)]
pub struct Answer {
    pub choice: Choice,
    #[schemars(extend("format" = "uuid"))]
    pub id: RequestId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "Vec<Pick>", length(min = 1))]
    pub picks: Option<Vec<Pick>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "String")]
    pub text: Option<String>,
    pub wait: String,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Change {
    pub added: u64,
    pub count: String,
    pub created: bool,
    #[schemars(length(max = 200))]
    pub lines: Vec<ChangeLines>,
    pub path: String,
    pub removed: u64,
    pub total: u64,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct ChangeLines {
    pub mark: ChangeLinesMark,
    #[schemars(length(max = 240))]
    pub text: String,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub enum ChangeLinesMark {
    #[serde(rename = " ")]
    Space,
    #[serde(rename = "+")]
    Plus,
    #[serde(rename = "-")]
    Minus,
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
#[serde(deny_unknown_fields)]
pub struct ChatRef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "AgentRef")]
    pub from: Option<AgentRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "ChatRefKind")]
    pub kind: Option<ChatRefKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "i64")]
    pub past: Option<i64>,
    pub with: Address,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub enum ChatRefKind {
    #[serde(rename = "letters")]
    Letters,
    #[serde(rename = "questions")]
    Questions,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(tag = "what", deny_unknown_fields)]
pub enum Check {
    #[serde(rename = "model")]
    Model {
        #[schemars(extend("format" = "uuid"))]
        id: RequestId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[serde(deserialize_with = "present")]
        #[schemars(with = "String")]
        model: Option<String>,
    },
    #[serde(rename = "connection")]
    Connection {
        #[schemars(extend("format" = "uuid"))]
        id: RequestId,
    },
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub enum Choice {
    #[serde(rename = "allow")]
    Allow,
    #[serde(rename = "always")]
    Always,
    #[serde(rename = "deny")]
    Deny,
    #[serde(rename = "go")]
    Go,
    #[serde(rename = "withdraw")]
    Withdraw,
    #[serde(rename = "reply")]
    Reply,
    #[serde(rename = "dismiss")]
    Dismiss,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum ClientMessage {
    #[serde(rename = "hello")]
    Hello(Hello),
    #[serde(rename = "say")]
    Say(Say),
    #[serde(rename = "give")]
    Give(Give),
    #[serde(rename = "give-part")]
    GivePart(GivePart),
    #[serde(rename = "answer")]
    Answer(Answer),
    #[serde(rename = "stop")]
    Stop(Stop),
    #[serde(rename = "rewind")]
    Rewind(Rewind),
    #[serde(rename = "new")]
    New(New),
    #[serde(rename = "resume")]
    Resume(Resume),
    #[serde(rename = "retry")]
    Retry(Retry),
    #[serde(rename = "edit")]
    Edit(Edit),
    #[serde(rename = "open")]
    Open(Open),
    #[serde(rename = "close")]
    Close(Close),
    #[serde(rename = "login")]
    Login(Login),
    #[serde(rename = "check")]
    Check(Check),
    #[serde(rename = "read")]
    Read(Read),
    #[serde(rename = "move-houses")]
    MoveHouses(MoveHouses),
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Close {
    pub chat: ChatRef,
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
pub struct Deed {
    pub description: String,
    pub name: String,
    pub usage: String,
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
#[serde(tag = "doc", deny_unknown_fields)]
pub enum Edit {
    #[serde(rename = "charter")]
    Charter {
        #[schemars(extend("format" = "uuid"))]
        id: RequestId,
        #[serde(deserialize_with = "Option::deserialize")]
        text: Option<String>,
    },
    #[serde(rename = "character")]
    Character {
        agent: Address,
        #[schemars(extend("format" = "uuid"))]
        id: RequestId,
        text: String,
    },
    #[serde(rename = "notes")]
    Notes {
        agent: Address,
        #[schemars(extend("format" = "uuid"))]
        id: RequestId,
        text: String,
    },
    #[serde(rename = "limits")]
    Limits(EditLimits),
    #[serde(rename = "settings")]
    Settings {
        #[schemars(extend("format" = "uuid"))]
        id: RequestId,
        set: EditSettingsSet,
    },
    #[serde(rename = "catalogue")]
    Catalogue {
        #[schemars(extend("format" = "uuid"))]
        id: RequestId,
        model: String,
        set: EditCatalogueSet,
    },
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct EditLimitsSetSet {
    pub key: String,
    pub section: String,
    pub value: serde_json::Value,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(untagged, deny_unknown_fields)]
pub enum EditLimits {
    Set {
        hamlet: String,
        #[schemars(extend("format" = "uuid"))]
        id: RequestId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[serde(deserialize_with = "present")]
        #[schemars(with = "String")]
        reason: Option<String>,
        set: EditLimitsSetSet,
    },
    Whole {
        hamlet: String,
        #[schemars(extend("format" = "uuid"))]
        id: RequestId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[serde(deserialize_with = "present")]
        #[schemars(with = "String")]
        reason: Option<String>,
        whole: std::collections::BTreeMap<String, serde_json::Value>,
    },
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct EditSettingsSet {
    #[serde(rename = "allowNet")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "bool")]
    pub allow_net: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "Vec<Endpoint>")]
    pub endpoints: Option<Vec<Endpoint>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "String")]
    pub model: Option<String>,
    #[serde(rename = "reflectEvery")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "i64")]
    pub reflect_every: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "Option<EditSettingsSetWorkspace>")]
    pub workspace: Option<Option<EditSettingsSetWorkspace>>,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct EditSettingsSetWorkspace {
    pub token: String,
    pub url: String,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct EditCatalogueSet {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "bool")]
    pub allowed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "String")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "Option<Price>")]
    pub price: Option<Option<Price>>,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Endpoint {
    #[serde(rename = "baseUrl")]
    pub base_url: String,
    pub name: String,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Fault {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "RequestId", extend("format" = "uuid"))]
    pub to: Option<RequestId>,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(untagged, deny_unknown_fields)]
pub enum Give {
    Files {
        files: Vec<GiveFilesFiles>,
        #[schemars(extend("format" = "uuid"))]
        id: RequestId,
        to: Address,
    },
    Skill {
        #[schemars(extend("format" = "uuid"))]
        id: RequestId,
        skill: GiveSkillSkill,
        to: Address,
    },
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct GiveFilesFiles {
    pub path: String,
    #[schemars(range(max = 9_007_199_254_740_991_u64))]
    pub size: u64,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(untagged, deny_unknown_fields)]
pub enum GiveSkillSkill {
    Starter { starter: String },
    From { from: Address, name: String },
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct GivePart {
    pub data: String,
    #[schemars(extend("format" = "uuid"))]
    pub id: RequestId,
    pub offset: u64,
    pub path: String,
    pub upload: String,
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
#[serde(deny_unknown_fields)]
pub struct Hello {
    pub token: String,
    #[schemars(extend("const" = V))]
    pub v: u64,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Item {
    pub at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "String")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "String")]
    pub chief: Option<String>,
    pub id: String,
    pub kind: ItemKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "bool")]
    pub rewindable: Option<bool>,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "ItemTool")]
    pub tool: Option<ItemTool>,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub enum ItemKind {
    #[serde(rename = "user")]
    User,
    #[serde(rename = "assistant")]
    Assistant,
    #[serde(rename = "reasoning")]
    Reasoning,
    #[serde(rename = "tool")]
    Tool,
    #[serde(rename = "notice")]
    Notice,
    #[serde(rename = "error")]
    Error,
    #[serde(rename = "answered")]
    Answered,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct ItemTool {
    #[serde(deserialize_with = "Option::deserialize")]
    pub change: Option<Change>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub exit: Option<i64>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub failure: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub ms: Option<u64>,
    pub name: String,
    pub summary: String,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct LastActivity {
    pub at: u64,
    pub what: String,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub enum Lasting {
    #[serde(rename = "session")]
    Session,
    #[serde(rename = "grant")]
    Grant,
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
pub struct LibrarySkill {
    pub by: String,
    pub description: String,
    pub name: String,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub asking: LimitsAsking,
    pub budget: LimitsBudget,
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
pub struct Link {
    #[serde(deserialize_with = "Option::deserialize")]
    pub models: Option<i64>,
    pub state: LinkState,
    #[serde(deserialize_with = "Option::deserialize")]
    pub words: Option<String>,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub enum LinkState {
    #[serde(rename = "untested")]
    Untested,
    #[serde(rename = "testing")]
    Testing,
    #[serde(rename = "connected")]
    Connected,
    #[serde(rename = "failed")]
    Failed,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Listed {
    pub allowed: bool,
    #[serde(deserialize_with = "Option::deserialize")]
    pub context: Option<i64>,
    pub name: String,
    pub note: String,
    #[serde(deserialize_with = "Option::deserialize")]
    pub price: Option<Price>,
    pub priced: ListedPriced,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub enum ListedPriced {
    #[serde(rename = "unknown")]
    Unknown,
    #[serde(rename = "listed")]
    Listed,
    #[serde(rename = "assigned")]
    Assigned,
    #[serde(rename = "yours")]
    Yours,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(untagged, deny_unknown_fields)]
pub enum Login {
    Start {
        #[schemars(extend("format" = "uuid"))]
        id: RequestId,
        start: LoginStartStart,
    },
    Reply {
        #[schemars(extend("format" = "uuid"))]
        id: RequestId,
        reply: LoginReplyReply,
    },
    Cancel {
        cancel: String,
        #[schemars(extend("format" = "uuid"))]
        id: RequestId,
    },
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct LoginStartStart {
    pub method: LoginMethod,
    pub provider: String,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct LoginReplyReply {
    pub login: String,
    pub value: String,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub enum LoginMethod {
    #[serde(rename = "key")]
    Key,
    #[serde(rename = "subscription")]
    Subscription,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct LoginProgress {
    pub login: String,
    pub step: LoginStep,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "RequestId", extend("format" = "uuid"))]
    pub to: Option<RequestId>,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(untagged, deny_unknown_fields)]
pub enum LoginStep {
    Url {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[serde(deserialize_with = "present")]
        #[schemars(with = "String")]
        instructions: Option<String>,
        url: String,
    },
    Code {
        code: String,
    },
    Prompt {
        prompt: String,
        secret: bool,
    },
    Progress {
        progress: String,
    },
    Done {
        done: String,
    },
    Failed {
        failed: String,
    },
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct MoveHouses {
    #[schemars(extend("format" = "uuid"))]
    pub id: RequestId,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct New {
    #[schemars(extend("format" = "uuid"))]
    pub id: RequestId,
    pub to: Address,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Offer {
    pub description: String,
    pub title: String,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(tag = "op", deny_unknown_fields)]
pub enum Op {
    #[serde(rename = "add")]
    Add {
        item: Item,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[serde(deserialize_with = "present")]
        #[schemars(with = "String")]
        replaces: Option<String>,
    },
    #[serde(rename = "append")]
    Append { item: String, text: String },
    #[serde(rename = "close")]
    Close {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[serde(deserialize_with = "present")]
        #[schemars(with = "Option<Change>")]
        change: Option<Option<Change>>,
        exit: i64,
        #[serde(deserialize_with = "Option::deserialize")]
        failure: Option<String>,
        item: String,
        #[serde(deserialize_with = "Option::deserialize")]
        ms: Option<u64>,
    },
    #[serde(rename = "status")]
    Status {
        #[serde(deserialize_with = "Option::deserialize")]
        last: Option<LastActivity>,
        pending: u64,
        working: bool,
    },
    #[serde(rename = "usage")]
    Usage { usage: UsageView },
    #[serde(rename = "reset")]
    Reset {},
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Open {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "String")]
    pub before: Option<String>,
    pub chat: ChatRef,
    #[schemars(extend("format" = "uuid"))]
    pub id: RequestId,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Papers {
    pub character: String,
    pub notes: String,
    pub skills: Vec<Skill>,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Past {
    pub began: i64,
    pub by: String,
    pub current: bool,
    pub messages: i64,
    pub modified: i64,
    pub number: i64,
    pub title: String,
    pub tokens: u64,
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

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum Pick {
    #[serde(rename = "offer")]
    Offer { index: u64 },
    #[serde(rename = "words")]
    Words { text: String },
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Price {
    pub input: f64,
    pub output: f64,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct ProviderView {
    pub configured: bool,
    pub id: String,
    pub label: String,
    pub methods: Vec<LoginMethod>,
    pub name: String,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Question {
    pub description: String,
    pub options: Vec<Offer>,
    pub question: String,
    pub text_input: bool,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Questions {
    pub message: String,
    #[schemars(length(min = 1))]
    pub questions: Vec<Question>,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(tag = "what", deny_unknown_fields)]
pub enum Read {
    #[serde(rename = "prompt")]
    Prompt {
        agent: Address,
        #[schemars(extend("format" = "uuid"))]
        id: RequestId,
    },
    #[serde(rename = "history")]
    History {
        agent: Address,
        #[schemars(extend("format" = "uuid"))]
        id: RequestId,
    },
    #[serde(rename = "models")]
    Models {
        #[schemars(extend("format" = "uuid"))]
        id: RequestId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[serde(deserialize_with = "present")]
        #[schemars(with = "bool")]
        refresh: Option<bool>,
    },
    #[serde(rename = "papers")]
    Papers {
        agent: Address,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[serde(deserialize_with = "present")]
        #[schemars(with = "bool")]
        full: Option<bool>,
        #[schemars(extend("format" = "uuid"))]
        id: RequestId,
    },
    #[serde(rename = "charter")]
    Charter {
        #[schemars(extend("format" = "uuid"))]
        id: RequestId,
    },
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(untagged, deny_unknown_fields)]
pub enum ReadResult {
    Prompt {
        prompt: String,
        #[schemars(extend("format" = "uuid"))]
        to: RequestId,
    },
    History {
        history: Vec<Past>,
        #[schemars(extend("format" = "uuid"))]
        to: RequestId,
    },
    Models {
        models: Vec<Listed>,
        #[schemars(extend("format" = "uuid"))]
        to: RequestId,
    },
    Papers {
        papers: Papers,
        #[schemars(extend("format" = "uuid"))]
        to: RequestId,
    },
    Charter {
        charter: ReadResultCharterCharter,
        #[schemars(extend("format" = "uuid"))]
        to: RequestId,
    },
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct ReadResultCharterCharter {
    pub own: bool,
    pub text: String,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    pub kind: ReplyKind,
    pub text: String,
    #[schemars(extend("format" = "uuid"))]
    pub to: RequestId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "String")]
    pub upload: Option<String>,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub enum ReplyKind {
    #[serde(rename = "notice")]
    Notice,
    #[serde(rename = "error")]
    Error,
    #[serde(rename = "rewound")]
    Rewound,
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

pub type RequestId = String;

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Resume {
    #[schemars(extend("format" = "uuid"))]
    pub id: RequestId,
    pub number: i64,
    pub to: Address,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Retry {
    #[schemars(extend("format" = "uuid"))]
    pub id: RequestId,
    pub to: Address,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Rewind {
    pub chat: ChatRef,
    #[schemars(extend("format" = "uuid"))]
    pub id: RequestId,
    pub item: String,
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
pub struct Say {
    #[schemars(extend("format" = "uuid"))]
    pub id: RequestId,
    pub text: String,
    pub to: Address,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum ServerMessage {
    #[serde(rename = "state")]
    State(State),
    #[serde(rename = "change")]
    Change(Changes),
    #[serde(rename = "turn")]
    Turn(Turn),
    #[serde(rename = "transcript")]
    Transcript(Transcript),
    #[serde(rename = "reply")]
    Reply(Reply),
    #[serde(rename = "login")]
    Login(LoginProgress),
    #[serde(rename = "read")]
    Read(ReadResult),
    #[serde(rename = "fault")]
    Fault(Fault),
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct SettingsView {
    #[serde(rename = "allowNet")]
    pub allow_net: bool,
    pub endpoints: Vec<SettingsViewEndpoints>,
    pub link: Link,
    pub model: String,
    #[serde(rename = "reflectEvery")]
    pub reflect_every: i64,
    pub unset: bool,
    pub workspace: SettingsViewWorkspace,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct SettingsViewEndpoints {
    #[serde(rename = "baseUrl")]
    pub base_url: String,
    pub key: bool,
    pub name: String,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct SettingsViewWorkspace {
    #[serde(rename = "move")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "SettingsViewWorkspaceMove")]
    pub move_: Option<SettingsViewWorkspaceMove>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub running: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub url: Option<String>,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct SettingsViewWorkspaceMove {
    pub done: i64,
    pub of: i64,
    pub state: SettingsViewWorkspaceMoveState,
    pub text: String,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub enum SettingsViewWorkspaceMoveState {
    #[serde(rename = "moving")]
    Moving,
    #[serde(rename = "moved")]
    Moved,
    #[serde(rename = "failed")]
    Failed,
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
pub struct Stop {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "Address")]
    pub agent: Option<Address>,
    #[schemars(extend("format" = "uuid"))]
    pub id: RequestId,
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
pub struct Transcript {
    pub chat: ChatRef,
    #[serde(deserialize_with = "Option::deserialize")]
    pub earlier: Option<String>,
    pub items: Vec<Item>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub last: Option<LastActivity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "i64")]
    pub number: Option<i64>,
    pub pending: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "RequestId", extend("format" = "uuid"))]
    pub to: Option<RequestId>,
    pub usage: UsageView,
    pub working: bool,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Turn {
    pub chat: ChatRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "present")]
    #[schemars(with = "i64")]
    pub number: Option<i64>,
    pub ops: Vec<Op>,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(tag = "type", deny_unknown_fields)]
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
pub struct Wait {
    pub about: String,
    #[serde(deserialize_with = "Option::deserialize")]
    pub agent: Option<AgentRef>,
    pub answers: Vec<Choice>,
    pub asked: u64,
    pub by: String,
    #[serde(deserialize_with = "Option::deserialize")]
    pub change: Option<Change>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub chat: Option<ChatRef>,
    pub detail: WaitDetail,
    pub id: String,
    pub since: u64,
    pub subject: String,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
pub enum WaitDanger {
    #[serde(rename = "network")]
    Network,
    #[serde(rename = "unconfined")]
    Unconfined,
    #[serde(rename = "destructive")]
    Destructive,
    #[serde(rename = "reserved")]
    Reserved,
    #[serde(rename = "edit")]
    Edit,
    #[serde(rename = "command")]
    Command,
}

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum WaitDetail {
    #[serde(rename = "approval")]
    Approval {
        #[serde(deserialize_with = "Option::deserialize")]
        always: Option<Lasting>,
        danger: WaitDanger,
        says: String,
    },
    #[serde(rename = "fault")]
    Fault {},
    #[serde(rename = "question")]
    Question { questions: Questions },
    #[serde(rename = "objection")]
    Objection {},
}

/// The value of every `v` field.
pub const V: u64 = 0;

fn present<'de, D, T>(
    deserializer: D,
) -> std::result::Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}
