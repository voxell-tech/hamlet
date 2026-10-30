//! The realm as the server shows it: the snapshot a connection begins
//! with and the changes that follow.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::client::{
    Limits, Period, Permission, ProviderView, SettingsView,
    TerminalView, Wait,
};

#[derive(
    Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct State {
    pub agents: Vec<Agent>,
    pub chief: String,
    pub closed: Vec<ClosedHamlet>,
    pub confinement: Confinement,
    pub deeds: Vec<Deed>,
    pub epoch: i64,
    pub grants: Vec<Grant>,
    pub hamlets: Vec<Hamlet>,
    pub houses: String,
    pub library: Vec<LibrarySkill>,
    pub providers: Vec<ProviderView>,
    pub revision: i64,
    pub settings: SettingsView,
    pub starters: Vec<Skill>,
    pub terminals: Vec<TerminalView>,
    pub waits: Vec<Wait>,
}

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
    /// A hamlet, an agent, a wait or a terminal replaces the one with
    /// its id in its place, or is added at the end when it is new.
    /// `AgentGone`, `WaitClosed` and `TerminalGone` remove theirs.
    /// Grants, settings and providers
    /// replace the whole value. The epoch and the revision are the
    /// caller's to keep.
    pub fn apply(&mut self, update: &Update) {
        match update {
            Update::Hamlet { hamlet } => {
                put(&mut self.hamlets, hamlet, |h| h.id == hamlet.id);
            }
            Update::Agent { agent } => {
                put(&mut self.agents, agent, |a| {
                    a.agent == agent.agent
                });
            }
            Update::AgentGone { agent } => {
                self.agents.retain(|a| a.agent != *agent);
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
            Update::Terminal { terminal } => {
                put(&mut self.terminals, terminal, |t| {
                    t.id == terminal.id
                });
            }
            Update::TerminalGone { id } => {
                self.terminals.retain(|t| t.id != *id);
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
        agent: AgentRef,
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
    #[serde(rename = "terminal")]
    Terminal { terminal: TerminalView },
    #[serde(rename = "terminal-gone")]
    TerminalGone { id: String },
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
    pub agent: AgentRef,
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
        state.hamlets.push(ours(realm::hamlet("orchard")));
        state.agents.push(ours(realm::agent("home", 1, "scout")));
        state.waits =
            vec![ours(realm::wait("a")), ours(realm::wait("b"))];
        state
    }

    #[test]
    fn a_changed_agent_keeps_its_place_and_a_new_one_comes_last() {
        let mut state = state();
        let mut founder = state.agents[0].clone();
        founder.busy = true;
        state.apply(&Update::Agent {
            agent: founder.clone(),
        });
        let guest =
            ours::<Agent>(realm::agent("orchard", 0, "guest"));
        state.apply(&Update::Agent {
            agent: guest.clone(),
        });
        let scout = ours(realm::agent("home", 1, "scout"));
        assert_eq!(state.agents, [founder, scout, guest]);
    }

    #[test]
    fn a_gone_agent_leaves_the_others_in_order() {
        let mut state = state();
        state.agents.push(ours(realm::agent("home", 2, "smith")));
        state.apply(&Update::AgentGone {
            agent: ours(realm::reference("home", 1)),
        });
        let names = state
            .agents
            .iter()
            .map(|a| a.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, ["founder", "smith"]);
    }
}
