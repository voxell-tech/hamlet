//! The calls the `realm` actor takes, and its answers.

pub mod budget;
pub mod client;
pub mod world;

use hamlet_actor::Action;
use hamlet_protocol::client::RequestId;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::address::{Agent, Realm, Workspace};
use crate::sender::Client;
use crate::workspace::files::EditDiff;
use crate::workspace::reach::{Need, SecretsVersion};
use crate::workspace::{
    AgentNumber, CallId, ConversationId, ReachVersion, RealmTask,
};
use crate::{Message, worded};

/// Asks the realm for its list of secrets in force. Called by the
/// workspace, which holds the answer in memory only.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Secrets;

impl Action for Secrets {
    type Output = SecretList;
    const NAME: &'static str = "secrets";
}

impl Message for Secrets {
    type To = Realm;
    type From = Workspace;
}

/// The realm's secrets of one version: the values the workspace takes
/// out of everything it hands to an agent.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretList {
    pub version: SecretsVersion,
    pub values: Vec<String>,
}

/// Names how many values there are, never a value.
impl std::fmt::Debug for SecretList {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        f.debug_struct("SecretList")
            .field("version", &self.version)
            .field("values", &self.values.len())
            .finish()
    }
}

/// Tells the realm that an agent's call waits for a chief, so it asks
/// one (H11). Called by the workspace once, when the wait opens. The
/// realm answers the workspace with a
/// [`crate::workspace::ChiefAnswer`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NeedsChief {
    pub agent: AgentNumber,
    pub call: CallId,
    pub conversation: ConversationId,
    /// The realm's task of the call, as the call carried it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub realm_task: Option<RealmTask>,
    pub needs: Vec<Need>,
    /// The call in words for the chief, the secrets taken out: a
    /// command's script, or a change's path and texts, each text cut
    /// to 2000 characters.
    pub subject: String,
    /// An edit's change, the secrets taken out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change: Option<EditDiff>,
    /// The agent's reach the call was weighed under.
    pub reach_version: ReachVersion,
}

impl Action for NeedsChief {
    type Output = ();
    const NAME: &'static str = "needs_chief";
}

impl Message for NeedsChief {
    type To = Realm;
    type From = Workspace;
}

/// A chief's request. A repeat carries the same `id`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Asked<T> {
    pub id: RequestId,
    pub act: T,
}

/// An agent's call of a society tool. A repeat carries the same
/// `call`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tool<T> {
    pub call: CallId,
    pub act: T,
}

/// An act the chief requests.
pub trait ChiefRequest:
    Serialize + DeserializeOwned + Send + Sync + 'static
{
    const NAME: &'static str;
    type Answer: Refusable
        + Serialize
        + DeserializeOwned
        + Send
        + 'static;
}

/// An act an agent calls as a society tool.
pub trait ToolCall:
    Serialize + DeserializeOwned + Send + Sync + 'static
{
    const TOOL: SocietyTool;
    type Answer: Refusable
        + Serialize
        + DeserializeOwned
        + Send
        + 'static;
}

/// An answer with an arm `refused`.
pub trait Refusable {
    type Why;
    fn refused(why: Self::Why) -> Self;
}

impl<T: ChiefRequest> Action for Asked<T> {
    type Output = T::Answer;
    const NAME: &'static str = T::NAME;
}

impl<T: ChiefRequest> Message for Asked<T> {
    type To = Realm;
    type From = Client;
}

impl<T: ToolCall> Action for Tool<T> {
    type Output = T::Answer;
    const NAME: &'static str = T::TOOL.name();
}

impl<T: ToolCall> Message for Tool<T> {
    type To = Realm;
    type From = Agent;
}

/// The answer of an act that gives back nothing but that it was done.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    bound(
        serialize = "W: Serialize + std::fmt::Display",
        deserialize = "W: Deserialize<'de>"
    )
)]
pub enum Done<W> {
    Done,
    #[serde(serialize_with = "worded")]
    Refused {
        why: W,
    },
}

impl<W> Refusable for Done<W> {
    type Why = W;
    fn refused(why: W) -> Self {
        Self::Refused { why }
    }
}

/// The society tools, by name (C16).
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum SocietyTool {
    Found,
    Hire,
    End,
    Seat,
    Tell,
    Ask,
    LayRoad,
    TakeUpRoad,
    RequestRoad,
    SetTools,
}

impl SocietyTool {
    /// Its name on the wire.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Found => "found",
            Self::Hire => "hire",
            Self::End => "end",
            Self::Seat => "seat",
            Self::Tell => "tell",
            Self::Ask => "ask",
            Self::LayRoad => "lay_road",
            Self::TakeUpRoad => "take_up_road",
            Self::RequestRoad => "request_road",
            Self::SetTools => "set_tools",
        }
    }
}

impl std::fmt::Display for SocietyTool {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// The calling agent's standing in the realm.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Standing {
    /// The calling agent has ended (H28).
    #[error("you have left: {how}{}", why.as_ref().map_or_else(String::new, |why| format!(": {why}")))]
    YouLeft {
        how: HowEnded,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        why: Option<String>,
    },
}

/// A tool a lead turned off for the caller (C16).
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Gate {
    #[error("your lead turned off the tool `{tool}` for you")]
    ToolOff { tool: SocietyTool },
}

/// An act that needs a lead's powers over the hire it names (H41).
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Rank {
    #[error("only a lead of `hire#{}` may do that", hire.0)]
    NotALead { hire: AgentNumber },
}

/// Why an agent's society tool was not carried out.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
#[serde(untagged)]
pub enum AgentRefusal<R> {
    #[error("{0}")]
    Standing(Standing),
    #[error("{0}")]
    Gate(Gate),
    #[error("{0}")]
    Act(R),
}

/// Why an agent's act that needs a lead's powers was not carried out.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
#[serde(untagged)]
pub enum LeadRefusal<R> {
    #[error("{0}")]
    Standing(Standing),
    #[error("{0}")]
    Gate(Gate),
    #[error("{0}")]
    Rank(Rank),
    #[error("{0}")]
    Act(R),
}

impl<R> From<Standing> for AgentRefusal<R> {
    fn from(why: Standing) -> Self {
        Self::Standing(why)
    }
}

impl<R> From<Gate> for AgentRefusal<R> {
    fn from(why: Gate) -> Self {
        Self::Gate(why)
    }
}

impl<R> From<Standing> for LeadRefusal<R> {
    fn from(why: Standing) -> Self {
        Self::Standing(why)
    }
}

impl<R> From<Gate> for LeadRefusal<R> {
    fn from(why: Gate) -> Self {
        Self::Gate(why)
    }
}

/// A hamlet's id, a UUID.
#[derive(
    Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub struct HamletId(pub String);

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub struct ProfileId(pub u32);

/// The realm's id of a question.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub struct LetterId(pub u32);

/// The most hires a hamlet holds (H6).
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    Fifty,
    Hundred,
    TwoHundred,
    FiveHundred,
}

/// An agent as a chief or an agent names it (H14). `Mayor` is the
/// office of the caller's hamlet.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Named {
    Hand,
    Mayor,
    MayorOf { hamlet: HamletId },
    Agent { agent: AgentNumber },
}

/// How and why an agent ended (H28).
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
#[error("{how}{}", why.as_ref().map_or_else(String::new, |why| format!(": {why}")))]
pub struct Left {
    pub how: HowEnded,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

/// Who ended it.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HowEnded {
    #[error("ended by the chief")]
    ByTheChief,
    #[error("ended by `hire#{}`", lead.0)]
    ByALead { lead: AgentNumber },
    #[error("ended by the Hand")]
    ByTheHand,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Found {
    pub name: String,
    /// The mayor's profile. Absent makes a blank one for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mayor: Option<ProfileId>,
    pub tier: Tier,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,
}

impl ChiefRequest for Found {
    const NAME: &'static str = "chief_found";
    type Answer = FoundAnswer<FoundRefusal>;
}

/// Only the Hand calls it.
impl ToolCall for Found {
    const TOOL: SocietyTool = SocietyTool::Found;
    type Answer = FoundAnswer<FoundRefusal>;
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    bound(
        serialize = "W: Serialize + std::fmt::Display",
        deserialize = "W: Deserialize<'de>"
    )
)]
pub enum FoundAnswer<W> {
    /// The mayor is still being hired.
    Founded {
        hamlet: HamletId,
        mayor: AgentNumber,
    },
    #[serde(serialize_with = "worded")]
    Refused { why: W },
}

impl<W> Refusable for FoundAnswer<W> {
    type Why = W;
    fn refused(why: W) -> Self {
        Self::Refused { why }
    }
}

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FoundRefusal {
    #[error("a hamlet needs a name")]
    EmptyName,
    #[error("there is no `profile#{}`", profile.0)]
    NoSuchProfile { profile: ProfileId },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hire {
    /// The hamlet, for the chief and the Hand. A hire's is its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hamlet: Option<HamletId>,
    /// The lead the hire answers to. Absent is the calling agent, or
    /// the mayor for the chief and the Hand.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub under: Option<Named>,
    /// Absent makes a blank profile for the hire (H38).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<ProfileId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,
}

impl ChiefRequest for Hire {
    const NAME: &'static str = "chief_hire";
    type Answer = HireAnswer<HireRefusal>;
}

impl ToolCall for Hire {
    const TOOL: SocietyTool = SocietyTool::Hire;
    type Answer = HireAnswer<LeadRefusal<HireRefusal>>;
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    bound(
        serialize = "W: Serialize + std::fmt::Display",
        deserialize = "W: Deserialize<'de>"
    )
)]
pub enum HireAnswer<W> {
    /// The hire is still being hired.
    Hired { agent: AgentNumber },
    #[serde(serialize_with = "worded")]
    Refused { why: W },
}

impl<W> Refusable for HireAnswer<W> {
    type Why = W;
    fn refused(why: W) -> Self {
        Self::Refused { why }
    }
}

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HireRefusal {
    #[error("say which hamlet")]
    NoHamlet,
    #[error("there is no such hamlet")]
    NoSuchHamlet,
    #[error("no agent is at that address")]
    NoSuchAgent,
    #[error("`hire#{}` is in another hamlet", lead.0)]
    OtherHamlet { lead: AgentNumber },
    #[error("`hire#{}` has left: {left}", lead.0)]
    LeadLeft { lead: AgentNumber, left: Left },
    #[error("the hamlet `{}` holds its most hires, {most}", hamlet.0)]
    Full { hamlet: HamletId, most: u32 },
    #[error("there is no `profile#{}`", profile.0)]
    NoSuchProfile { profile: ProfileId },
    #[error("say what it is for")]
    NoPurpose,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct End {
    pub hire: Named,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

impl ChiefRequest for End {
    const NAME: &'static str = "chief_end";
    type Answer = Done<EndRefusal>;
}

impl ToolCall for End {
    const TOOL: SocietyTool = SocietyTool::End;
    type Answer = Done<LeadRefusal<EndRefusal>>;
}

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EndRefusal {
    #[error("no agent is at that address")]
    NoSuchAgent,
    #[error("the Hand cannot be ended")]
    TheHand,
    #[error("a mayor cannot be ended; seat another mayor first")]
    TheMayor,
    #[error("`hire#{}` has left: {left}", hire.0)]
    Left { hire: AgentNumber, left: Left },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Seat {
    pub hire: Named,
}

impl ChiefRequest for Seat {
    const NAME: &'static str = "chief_seat";
    type Answer = Done<SeatRefusal>;
}

/// Only the Hand calls it.
impl ToolCall for Seat {
    const TOOL: SocietyTool = SocietyTool::Seat;
    type Answer = Done<SeatRefusal>;
}

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SeatRefusal {
    #[error("no agent is at that address")]
    NoSuchAgent,
    #[error("only a hire can be a mayor")]
    NotAHire,
    #[error("`hire#{}` has left: {left}", hire.0)]
    Left { hire: AgentNumber, left: Left },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tell {
    pub to: Named,
    pub text: String,
    /// The task of the input the sender's turn works on, which pays
    /// for the receiver's turn; absent for a turn that has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<RealmTask>,
}

impl ToolCall for Tell {
    const TOOL: SocietyTool = SocietyTool::Tell;
    type Answer = Done<AgentRefusal<LetterRefusal>>;
}

/// A question; its answer arrives later as an input to
/// `conversation`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ask {
    pub to: Named,
    pub text: String,
    pub conversation: ConversationId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<RealmTask>,
}

impl ToolCall for Ask {
    const TOOL: SocietyTool = SocietyTool::Ask;
    type Answer = Done<AgentRefusal<LetterRefusal>>;
}

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LetterRefusal {
    #[error("that is you")]
    Yourself,
    #[error("say what to send")]
    Empty,
    #[error("no agent is at that address")]
    NoSuchAgent,
    #[error("you have no road to `hire#{}`; ask your lead for one with request_road", to.0)]
    NoRoad { to: AgentNumber },
    #[error("no road yet to `hire#{}`: it is still being hired", to.0)]
    StillHiring { to: AgentNumber },
    #[error("`hire#{}` has left: {left}", to.0)]
    Left { to: AgentNumber, left: Left },
}

/// The answer to a question, when the asked agent's turn on it ends.
/// The question is its key, and a repeat keeps the first.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reply {
    pub question: LetterId,
    pub text: String,
}

impl Action for Reply {
    type Output = Done<ReplyRefusal>;
    const NAME: &'static str = "reply";
}

impl Message for Reply {
    type To = Realm;
    type From = Agent;
}

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ReplyRefusal {
    #[error("there is no such question")]
    NoSuchQuestion,
    #[error("the question was asked of another agent")]
    NotAsked,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayRoad {
    pub a: Named,
    pub b: Named,
}

impl ChiefRequest for LayRoad {
    const NAME: &'static str = "chief_lay_road";
    type Answer = Done<LayRefusal>;
}

impl ToolCall for LayRoad {
    const TOOL: SocietyTool = SocietyTool::LayRoad;
    type Answer = Done<LeadRefusal<LayRefusal>>;
}

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LayRefusal {
    #[error("no agent is at that address")]
    NoSuchAgent,
    #[error("a road joins two hires of one hamlet")]
    NotTwoHiresOfOneHamlet,
    #[error("`hire#{}` has left: {left}", hire.0)]
    Left { hire: AgentNumber, left: Left },
    #[error("`hire#{}` and `hire#{}` already have a road", a.0, b.0)]
    RoadExists { a: AgentNumber, b: AgentNumber },
    #[error("the hamlet `{}` has its most roads, {most}", hamlet.0)]
    Full { hamlet: HamletId, most: u32 },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TakeUpRoad {
    pub a: Named,
    pub b: Named,
}

impl ChiefRequest for TakeUpRoad {
    const NAME: &'static str = "chief_take_up_road";
    type Answer = Done<TakeUpRefusal>;
}

impl ToolCall for TakeUpRoad {
    const TOOL: SocietyTool = SocietyTool::TakeUpRoad;
    type Answer = Done<LeadRefusal<TakeUpRefusal>>;
}

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TakeUpRefusal {
    #[error("no agent is at that address")]
    NoSuchAgent,
    #[error("`hire#{}` and `hire#{}` have no road to take up", a.0, b.0)]
    NoRoad { a: AgentNumber, b: AgentNumber },
}

/// Asks the caller's lead for a road to `to`, as a letter to the
/// lead.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestRoad {
    pub to: Named,
    pub why: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<RealmTask>,
}

impl ToolCall for RequestRoad {
    const TOOL: SocietyTool = SocietyTool::RequestRoad;
    type Answer = Done<AgentRefusal<RequestRoadRefusal>>;
}

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RequestRoadRefusal {
    #[error("no agent is at that address")]
    NoSuchAgent,
    #[error("you have no lead to ask")]
    NoLead,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetTools {
    pub hire: Named,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub off: Vec<SocietyTool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub on: Vec<SocietyTool>,
}

impl ChiefRequest for SetTools {
    const NAME: &'static str = "chief_set_tools";
    type Answer = Done<ToolsRefusal>;
}

impl ToolCall for SetTools {
    const TOOL: SocietyTool = SocietyTool::SetTools;
    type Answer = Done<LeadRefusal<ToolsRefusal>>;
}

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolsRefusal {
    #[error("no agent is at that address")]
    NoSuchAgent,
    #[error("only a hire's tools are turned off")]
    NotAHire,
}

/// Raises or lowers a hamlet's tier (H6).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetTier {
    pub hamlet: HamletId,
    pub tier: Tier,
}

impl ChiefRequest for SetTier {
    const NAME: &'static str = "chief_set_tier";
    type Answer = Done<TierRefusal>;
}

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TierRefusal {
    #[error("there is no such hamlet")]
    NoSuchHamlet,
    #[error("the hamlet holds {living} hires, more than that tier")]
    TooSmall { living: u32 },
}

/// Sets limits of a hamlet, or of the realm itself (H22).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetLimits {
    /// Absent for the realm's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hamlet: Option<HamletId>,
    /// Set in order, so the last of one kind holds.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub budget: Vec<budget::BudgetLimit>,
}

impl ChiefRequest for SetLimits {
    const NAME: &'static str = "chief_set_limits";
    type Answer = Done<HamletRefusal>;
}

/// The refusal of a chief's act whose one condition is that the
/// hamlet it names exists.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HamletRefusal {
    #[error("there is no such hamlet")]
    NoSuchHamlet,
}
