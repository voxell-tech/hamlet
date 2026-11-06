//! The realm as the server shows it: the snapshot a connection begins
//! with and the changes that follow.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::client::{
    Limits, Period, Permission, ProviderView, RealmLimits,
    SettingsView, Wait,
};

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct State {
    /// The Hand, every hire and every worker, living and ended.
    pub agents: Vec<Agent>,
    pub chief: String,
    pub closed: Vec<ClosedHamlet>,
    pub confinement: Confinement,
    pub deeds: Vec<Deed>,
    pub epoch: i64,
    pub grants: Vec<Grant>,
    pub hamlets: Vec<Hamlet>,
    /// The Hand, the realm's own agent.
    pub hand: AgentId,
    pub library: Vec<LibrarySkill>,
    /// The realm's own limits.
    pub limits: RealmLimits,
    pub providers: Vec<ProviderView>,
    /// The realm's folder on the server, whose paths are never
    /// links.
    pub realm: String,
    pub revision: i64,
    /// The city's scholars, living and archived.
    pub scholars: Vec<Scholar>,
    pub settings: SettingsView,
    pub waits: Vec<Wait>,
}

/// An agent of the realm: the Hand, a hire or a worker.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    JsonSchema,
)]
#[serde(transparent)]
pub struct AgentId(pub u64);

/// A scholar of the city.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    JsonSchema,
)]
#[serde(transparent)]
pub struct ScholarId(pub u64);

/// What keeps the agents' commands from the rest of the machine.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum Confinement {
    /// A sandbox, which `name` names.
    #[serde(rename = "sandbox")]
    Sandbox { name: String },
    /// Nothing: the commands run with the server account's full
    /// reach.
    #[serde(rename = "insecure")]
    Insecure,
    /// The workspace is another machine, which holds the commands.
    #[serde(rename = "remote")]
    Remote,
}

impl Confinement {
    /// Whether the network switch keeps commands off the network.
    pub fn enforces_network(&self) -> bool {
        !matches!(self, Self::Insecure)
    }
}

impl State {
    /// Applies one change, as a client does between whole states.
    ///
    /// A hamlet, an agent, a scholar or a wait replaces the one with
    /// its id in its place, or is added at the end when it is new.
    /// `AgentGone`, `ScholarGone` and `WaitClosed` remove theirs.
    /// Grants, settings and providers replace the whole value. The
    /// epoch and the revision are the caller's to keep.
    pub fn apply(&mut self, update: &Update) {
        match update {
            Update::Hamlet { hamlet } => {
                put(&mut self.hamlets, hamlet, |h| h.id == hamlet.id);
            }
            Update::Agent { agent } => {
                put(&mut self.agents, agent, |a| a.id == agent.id);
            }
            Update::AgentGone { id } => {
                self.agents.retain(|a| a.id != *id);
            }
            Update::Scholar { scholar } => {
                put(&mut self.scholars, scholar, |s| {
                    s.id == scholar.id
                });
            }
            Update::ScholarGone { id } => {
                self.scholars.retain(|s| s.id != *id);
            }
            Update::Wait { wait } => {
                put(&mut self.waits, wait, |w| w.id == wait.id);
            }
            Update::WaitClosed { id, .. } => {
                self.waits.retain(|w| w.id != *id);
            }
            Update::Grants { grants } => {
                self.grants.clone_from(grants);
            }
            Update::Settings { settings } => {
                self.settings.clone_from(settings);
            }
            Update::Providers { providers } => {
                self.providers.clone_from(providers);
            }
        }
    }
}

/// Replaces the item of `items` that `same` finds with `item`, or
/// adds `item` at the end.
fn put<T: Clone>(
    items: &mut Vec<T>,
    item: &T,
    same: impl Fn(&T) -> bool,
) {
    match items.iter_mut().find(|old| same(old)) {
        Some(old) => old.clone_from(item),
        None => items.push(item.clone()),
    }
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
pub enum Update {
    #[serde(rename = "hamlet")]
    Hamlet { hamlet: Hamlet },
    #[serde(rename = "agent")]
    Agent { agent: Agent },
    /// An agent erased for good.
    #[serde(rename = "agent-gone")]
    AgentGone { id: AgentId },
    #[serde(rename = "scholar")]
    Scholar { scholar: Scholar },
    /// A scholar erased for good.
    #[serde(rename = "scholar-gone")]
    ScholarGone { id: ScholarId },
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
pub struct Agent {
    pub activity: AgentActivity,
    pub busy: bool,
    pub consulting: bool,
    pub conversations: Vec<ConversationView>,
    /// How hard it thinks when chosen for it; none while it follows
    /// the realm's `effort` setting.
    #[serde(deserialize_with = "Option::deserialize")]
    pub effort: Option<crate::Effort>,
    pub faulted: bool,
    /// Whether its calls ask for the provider's fast tier.
    pub fast: bool,
    /// A hire's hamlet, or a worker's when it has one.
    #[serde(deserialize_with = "Option::deserialize")]
    pub hamlet: Option<String>,
    pub id: AgentId,
    pub kind: AgentKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "Left")]
    pub left: Option<Left>,
    pub mayor: bool,
    /// The model it runs now.
    pub model: String,
    /// How it is named: `hand`, `name@hamlet`, `name#number@hamlet`
    /// or `worker#id`.
    pub name: String,
    /// What a hire is called in the world, given by a chief or an
    /// agent above it; none while it goes by its name.
    #[serde(deserialize_with = "Option::deserialize")]
    pub nickname: Option<String>,
    /// A hire's number in its hamlet.
    #[serde(deserialize_with = "Option::deserialize")]
    pub number: Option<u64>,
    /// The model chosen for it; none while it follows its hamlet's,
    /// or the realm's default.
    #[serde(rename = "ownModel")]
    #[serde(deserialize_with = "Option::deserialize")]
    pub own_model: Option<String>,
    /// The agent it answers to: a hire's lead, the Hand for a mayor,
    /// a worker's starter. None for the Hand, or a chief's worker.
    #[serde(deserialize_with = "Option::deserialize")]
    pub parent: Option<AgentId>,
    /// The version of its scholar a hire is pinned to; none while it
    /// follows `main`.
    #[serde(deserialize_with = "Option::deserialize")]
    pub pin: Option<String>,
    pub purpose: String,
    pub saying: AgentSaying,
    /// The scholar a hire is of, or the Hand's own knowledge.
    #[serde(deserialize_with = "Option::deserialize")]
    pub scholar: Option<ScholarId>,
    pub since: u64,
    pub skills: Vec<String>,
    pub spent: AgentSpent,
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
pub enum AgentKind {
    #[serde(rename = "hand")]
    Hand,
    #[serde(rename = "hire")]
    Hire,
    #[serde(rename = "worker")]
    Worker,
}

/// A scholar of the city: a repository of its character, memory and
/// skills.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Scholar {
    /// Whether it lives in the city or waits at its harbor.
    pub arrival: Arrival,
    /// Who last let it in, kept it waiting or turned it away: a
    /// chief's name or `hand`. None while it is as it was made.
    #[serde(rename = "arrivalBy")]
    #[serde(deserialize_with = "Option::deserialize")]
    pub arrival_by: Option<String>,
    /// The chief it belongs to.
    pub chief: String,
    /// Its living hires and those that may come back.
    pub hires: Vec<AgentId>,
    /// Where its house stands in the city, from the city's middle;
    /// none while it is not let in.
    #[serde(deserialize_with = "Option::deserialize")]
    pub house: Option<HouseAt>,
    pub id: ScholarId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "crate::client::present")]
    #[schemars(with = "ScholarLeft")]
    pub left: Option<ScholarLeft>,
    pub name: String,
    pub since: u64,
    pub skills: Vec<String>,
    pub versions: Vec<VersionView>,
}

/// Where a scholar stands with the city.
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
pub enum Arrival {
    /// It lives in its house in the city.
    #[serde(rename = "housed")]
    Housed,
    /// It waits at the city's harbor to be let in.
    #[serde(rename = "waiting")]
    Waiting,
    /// It was turned away, and waits at the harbor until a chief lets
    /// it in.
    #[serde(rename = "turned")]
    Turned,
}

/// A house's spot from the city's middle, and the way its door faces,
/// in radians about the vertical.
#[derive(
    Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct HouseAt {
    pub turn: f64,
    pub x: f64,
    pub z: f64,
}

impl HouseAt {
    /// The round at the city's middle that its landmarks keep: the
    /// Hand's tower, and the Hearth and the Library on the ring about
    /// it, which the world sets 44 m out with gardens 8.5 m about
    /// them (`city_ring` and `grounds` in `assets/world.toml`).
    pub const MIDDLE: f64 = 58.0;

    /// The least distance between the middles of two houses.
    pub const APART: f64 = 12.0;

    /// Whether a house at `x, z` from the city's middle keeps clear
    /// of the middle and of every house at `taken`.
    pub fn is_clear(
        taken: impl IntoIterator<Item = (f64, f64)>,
        x: f64,
        z: f64,
    ) -> bool {
        x.hypot(z) >= Self::MIDDLE + Self::APART * 0.5
            && taken
                .into_iter()
                .all(|(tx, tz)| (x - tx).hypot(z - tz) >= Self::APART)
    }
}

/// A tag of a scholar's history.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct VersionView {
    pub at: u64,
    pub name: String,
    pub note: String,
}

/// How a scholar left the city.
#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct ScholarLeft {
    pub at: u64,
    pub by: String,
    pub reason: String,
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
    /// The agent the conversation is from.
    pub with: AgentId,
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
    /// The hire it was merged into.
    #[serde(deserialize_with = "Option::deserialize")]
    pub into: Option<AgentId>,
    pub reason: String,
    pub words: String,
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
pub enum LeftHow {
    /// A lead or a chief ended the hire.
    #[serde(rename = "ended")]
    Ended,
    #[serde(rename = "merged")]
    Merged,
    /// Its scholar left the city.
    #[serde(rename = "scholar")]
    Scholar,
    /// Its hamlet was removed.
    #[serde(rename = "hamlet")]
    Hamlet,
    /// The worker answered its task.
    #[serde(rename = "answered")]
    Answered,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "stopped")]
    Stopped,
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
    /// The chiefs it answers to.
    pub chiefs: Vec<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub folder: Option<String>,
    pub id: String,
    pub limits: Limits,
    /// The hire at the root of its tree.
    pub mayor: AgentId,
    pub mode: HamletMode,
    #[serde(rename = "modeWords")]
    pub mode_words: String,
    pub name: String,
    pub roads: Vec<Road>,
    /// The most hires it holds.
    pub tier: u64,
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
    pub a: AgentId,
    pub b: AgentId,
    pub kind: RoadKind,
    #[serde(rename = "lastFrom")]
    #[serde(deserialize_with = "Option::deserialize")]
    pub last_from: Option<AgentId>,
    pub level: RoadLevel,
    pub messages: u64,
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
pub enum RoadKind {
    /// A hire and its lead, or the Hand and a mayor.
    #[serde(rename = "tree")]
    Tree,
    /// Two mayors.
    #[serde(rename = "mayors")]
    Mayors,
    /// Laid between two hires of a hamlet.
    #[serde(rename = "laid")]
    Laid,
    /// Agreed between hires of two hamlets.
    #[serde(rename = "agreed")]
    Agreed,
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
pub struct Grant {
    #[serde(deserialize_with = "Option::deserialize")]
    pub agent: Option<AgentId>,
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

#[cfg(test)]
mod tests {
    use hamlet_testing::realm;
    use serde::de::DeserializeOwned;

    use super::*;

    /// `value`, a fixture of the protocol crate `hamlet_testing`
    /// builds on, as this build's type.
    fn ours<T: DeserializeOwned>(value: impl Serialize) -> T {
        serde_json::from_value(serde_json::to_value(value).unwrap())
            .unwrap()
    }

    fn state() -> State {
        let mut state = ours::<State>(realm::state());
        state.hamlets.push(ours(realm::hamlet("orchard", 3)));
        state.agents.push(ours(realm::agent(3, "home", 1, "scout")));
        state.waits =
            vec![ours(realm::wait("a")), ours(realm::wait("b"))];
        state
    }

    #[test]
    fn a_changed_agent_keeps_its_place_and_a_new_one_comes_last() {
        let mut state = state();
        let mut hand = state.agents[0].clone();
        hand.busy = true;
        state.apply(&Update::Agent {
            agent: hand.clone(),
        });
        let guest =
            ours::<Agent>(realm::agent(9, "orchard", 0, "guest"));
        state.apply(&Update::Agent {
            agent: guest.clone(),
        });
        let mayor = state.agents[1].clone();
        let scout = ours(realm::agent(3, "home", 1, "scout"));
        assert_eq!(state.agents, [hand, mayor, scout, guest]);
    }

    #[test]
    fn a_gone_agent_leaves_the_others_in_order() {
        let mut state = state();
        state.agents.push(ours(realm::agent(4, "home", 2, "smith")));
        state.apply(&Update::AgentGone { id: AgentId(3) });
        let names = state
            .agents
            .iter()
            .map(|a| a.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, ["hand", "ada@home", "smith@home"]);
    }
}
