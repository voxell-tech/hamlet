//! The calls the `realm` actor takes, and its answers.

pub mod budget;
pub mod choices;
pub mod client;
pub mod documents;
pub mod hamlets;
pub mod models;
pub mod oversight;
pub mod parcels;
pub mod profiles;
pub mod prompt;
pub mod workers;
pub mod world;

use std::path::PathBuf;

use hamlet_actor::Action;
use hamlet_protocol::client::RequestId;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::address::{Agent, Realm, Workspace};
use crate::profile::VersionName;
use crate::sender::{Client, Scrubber};
use crate::workspace::files::EditDiff;
use crate::workspace::reach::{Need, SecretsVersion};
use crate::workspace::{
    AgentNumber, CallId, ConversationId, ReachVersion, RealmTask,
};
use crate::{Message, worded};

/// Asks the realm for its list of secrets in force. Called by the
/// workspace and by an agent, which hold the answer in memory only.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Secrets;

impl Action for Secrets {
    type Output = SecretList;
    const NAME: &'static str = "secrets";
}

impl Message for Secrets {
    type To = Realm;
    type From = Scrubber;
}

/// The realm's secrets of one version: the values the workspace takes
/// out of everything it hands to an agent, and an agent out of what a
/// client or a model reads.
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
    /// The realm's task of the calling turn, as the turn carried it:
    /// it pays for a letter's turn and names a reserved act's wait.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<RealmTask>,
    /// The calling turn's conversation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation: Option<ConversationId>,
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

answer!(
    /// The answer of an act on the agent `agent`.
    DoneTo<W> {
        Done { agent: AgentName },
    }
);

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
    AskChief,
    Object,
    StartWorker,
    SetAllowance,
    ReadBudget,
    AgreeRoad,
    SetNickname,
    SetModel,
    ReadRecord,
    CreateProfile,
    ReadProfile,
    EditProfile,
    Tag,
    Revert,
    AskLibrarian,
    SubmitSkill,
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
            Self::AskChief => "ask_chief",
            Self::Object => "object",
            Self::StartWorker => "start_worker",
            Self::SetAllowance => "set_allowance",
            Self::ReadBudget => "read_budget",
            Self::AgreeRoad => "agree_road",
            Self::SetNickname => "set_nickname",
            Self::SetModel => "set_model",
            Self::ReadRecord => "read_record",
            Self::CreateProfile => "create_profile",
            Self::ReadProfile => "read_profile",
            Self::EditProfile => "edit_profile",
            Self::Tag => "tag",
            Self::Revert => "revert",
            Self::AskLibrarian => "ask_librarian",
            Self::SubmitSkill => "submit_skill",
        }
    }
}

/// A tool by its name: a society tool's is [`SocietyTool::name`], and
/// any other is a built-in or a hand tool's. A list holds it as a
/// map, so a name that begins with `$` crosses the agent's host as
/// itself.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolName {
    pub name: String,
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
    #[error("you are not an agent of the realm's tree")]
    NotAMember,
}

/// Why a caller's tool does not run now: a chief must allow it, or a
/// chief's refusals refuse it (H11).
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
    /// The realm keeps the request for a chief: it runs if one
    /// allows it, and the caller is told.
    #[error("This call waits for a chief's answer.")]
    Reserved { wait: world::WaitId },
    /// A chief refused the same request on the caller's task, or
    /// every request for the network.
    #[error(
        "the chief already refused this in this turn ({reason}). Do \
         not ask again: go on without it, or report that you could \
         not."
    )]
    AlreadyRefused { reason: String },
    /// The chief refused as many requests on the caller's task as its
    /// limits allow.
    #[error(
        "the chief has refused {refused} requests this turn and \
         will not be asked again in it. Stop and report what you \
         could not do."
    )]
    PastTheCount { refused: u32 },
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
    #[error("only a lead of `{hire}` may do that")]
    NotALead { hire: AgentName },
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
    Worker { worker: AgentNumber },
}

/// The address an agent writes for it.
impl std::fmt::Display for Named {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        match self {
            Self::Hand => f.write_str("hand"),
            Self::Mayor => f.write_str("mayor"),
            Self::MayorOf { hamlet } => {
                write!(f, "mayor@{}", hamlet.0)
            }
            Self::Agent { agent } => write!(f, "hire#{}", agent.0),
            Self::Worker { worker } => {
                write!(f, "worker#{}", worker.0)
            }
        }
    }
}

/// An agent as chiefs and agents are shown it: its address, and
/// beside it a hire's nickname or its profile's name, or a worker's
/// nickname, as in `hire#3 (Mira)`; an erased hire, the Hand and a
/// nameless worker show their address alone.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentName {
    pub agent: AgentNumber,
    pub name: String,
}

impl std::fmt::Display for AgentName {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        f.write_str(&self.name)
    }
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
#[error("{how}")]
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
    #[error("it was ended")]
    ByTheChief,
    #[error("it was ended")]
    ByALead { lead: AgentName },
    #[error("it was ended")]
    ByTheHand,
    /// A worker answered its starter.
    #[error("it answered its task")]
    Answered,
    /// A worker's turn failed.
    #[error("its task failed")]
    Failed,
    /// A worker was stopped: with its starter, or by a chief.
    #[error("it was stopped")]
    Stopped,
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
    #[error(
        "`{name}` is not a name: use one line of 1 to 40 characters \
         with no colon"
    )]
    BadName { name: String },
    #[error("no scholar has the address `scholar#{}`", profile.0)]
    NoSuchProfile { profile: ProfileId },
    #[error("say what it is for")]
    NoPurpose,
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
    /// The profile's version the hire starts from and stays at;
    /// absent, it follows `main`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<VersionName>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,
    /// A blank hire's name; absent with a profile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
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
    Hired { agent: AgentName },
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
    #[error("Say which hamlet to hire into.")]
    NoHamlet,
    #[error("no hamlet has the id `{}`", id.0)]
    NoSuchHamlet { id: HamletId },
    #[error("no agent has the address `{address}`")]
    NoSuchAgent { address: String },
    #[error("`{lead}` is in another hamlet")]
    OtherHamlet { lead: AgentName },
    /// The lead named is the Hand or a worker.
    #[error("`{name}` is not a hire")]
    NotAHire { name: String },
    #[error("`{lead}` has left: {left}")]
    LeadLeft { lead: AgentName, left: Left },
    #[error("the hamlet `{}` holds its most hires, {most}", hamlet.0)]
    Full { hamlet: HamletId, most: u32 },
    #[error("no scholar has the address `scholar#{}`", profile.0)]
    NoSuchProfile { profile: ProfileId },
    /// `label` is `scholar#<id> (<name>)`.
    #[error(
        "`{label}` is a blank hire's own, not a scholar of the city"
    )]
    Apart { label: String },
    /// `label` is `scholar#<id> (<name>)`.
    #[error("`{label}` has no version `{}`", version.0)]
    NoVersion { label: String, version: VersionName },
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
    type Answer = DoneTo<LeadRefusal<EndRefusal>>;
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
    #[error("no agent has the address `{address}`")]
    NoSuchAgent { address: String },
    #[error("`hand` is not a hire")]
    TheHand,
    #[error("a mayor cannot be ended; seat another mayor first")]
    TheMayor,
    #[error("`{hire}` has left: {left}")]
    Left { hire: AgentName, left: Left },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Seat {
    pub hire: Named,
    /// Why, for the record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

impl ChiefRequest for Seat {
    const NAME: &'static str = "chief_seat";
    type Answer = Done<OnlyAHire>;
}

/// Only the Hand calls it.
impl ToolCall for Seat {
    const TOOL: SocietyTool = SocietyTool::Seat;
    type Answer = Seated;
}

answer!(
    /// The answer of a [`Seat`]: the hire seated and the hamlet it is
    /// the mayor of.
    Seated<OnlyAHire> {
        Done { agent: AgentName, hamlet: HamletId },
    }
);

/// The refusal of an act that needs a living hire.
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
pub enum OnlyAHire {
    #[error("no agent has the address `{address}`")]
    NoSuchAgent { address: String },
    #[error("`{name}` is not a hire")]
    NotAHire { name: String },
    #[error("`{hire}` has left: {left}")]
    Left { hire: AgentName, left: Left },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tell {
    pub to: Named,
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<Enclosed>,
}

/// A file or a folder a letter sends, in the sender's filesystem
/// or its hamlet's folder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Enclosed {
    pub path: PathBuf,
}

impl ToolCall for Tell {
    const TOOL: SocietyTool = SocietyTool::Tell;
    type Answer = Sent<AgentRefusal<LetterRefusal>>;
}

answer!(
    /// The answer of a [`Tell`].
    Sent<W> {
        Sent { to: AgentName },
    }
);

/// A question; its answer arrives later as an input to
/// `conversation`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ask {
    pub to: Named,
    pub text: String,
    pub conversation: ConversationId,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<Enclosed>,
}

impl ToolCall for Ask {
    const TOOL: SocietyTool = SocietyTool::Ask;
    type Answer = Waiting<AgentRefusal<LetterRefusal>>;
}

answer!(
    /// The answer of an [`Ask`]: `question` is the letter its answer
    /// names.
    Waiting<W> {
        Waiting { on: AgentName, question: LetterId },
    }
);

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
    #[error("That is you.")]
    Yourself,
    #[error("Say what to send.")]
    Empty,
    #[error("no agent has the address `{address}`")]
    NoSuchAgent { address: String },
    #[error(
        "You have no road to {to}. Ask your lead for one with \
         request_road."
    )]
    NoRoad { to: AgentName },
    #[error("no road yet to `{to}`: it is still being hired")]
    StillHiring { to: AgentName },
    #[error(
        "The chief withdrew the instruction, so this call did not \
         run."
    )]
    Withdrawn,
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
    /// Why, for the record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

impl ChiefRequest for LayRoad {
    const NAME: &'static str = "chief_lay_road";
    type Answer = Done<LayRefusal>;
}

impl ToolCall for LayRoad {
    const TOOL: SocietyTool = SocietyTool::LayRoad;
    type Answer = RoadDone<LeadRefusal<LayRefusal>>;
}

answer!(
    /// The answer of an act on the road between `a` and `b`.
    RoadDone<W> {
        Done { a: AgentName, b: AgentName },
    }
);

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
    #[error("no agent has the address `{address}`")]
    NoSuchAgent { address: String },
    #[error("a road needs two different hires")]
    SameEnds,
    #[error("a road joins hires only")]
    NotHires,
    #[error("`{hire}` is in another hamlet")]
    OtherHamlet { hire: AgentName },
    #[error("`{hire}` has left: {left}")]
    Left { hire: AgentName, left: Left },
    #[error("`{a}` and `{b}` already have a road")]
    RoadExists { a: AgentName, b: AgentName },
    #[error("the hamlet `{}` has its most roads, {most}", hamlet.0)]
    Full { hamlet: HamletId, most: u32 },
}

/// A mayor's half of a road between a hire of its hamlet and a hire
/// of another. The road is laid once the other hamlet's mayor agrees
/// to it too.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgreeRoad {
    pub mine: Named,
    pub theirs: Named,
    /// Why, for the record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

impl ToolCall for AgreeRoad {
    const TOOL: SocietyTool = SocietyTool::AgreeRoad;
    type Answer = Agreed<AgentRefusal<AgreeRefusal>>;
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
pub enum Agreed<W> {
    /// The other mayor has not agreed yet.
    Waiting {
        mine: AgentName,
        theirs: AgentName,
    },
    Laid {
        mine: AgentName,
        theirs: AgentName,
    },
    #[serde(serialize_with = "worded")]
    Refused {
        why: W,
    },
}

impl<W> Refusable for Agreed<W> {
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
pub enum AgreeRefusal {
    #[error("only a mayor may do that")]
    NotAMayor,
    #[error("no agent has the address `{address}`")]
    NoSuchAgent { address: String },
    #[error("the chief allows no roads between hamlets")]
    NoRoadsBetween,
    #[error("a road joins hires only")]
    NotHires,
    #[error(
        "both ends of an agreed road must be in different hamlets"
    )]
    SameHamlet,
    #[error("`{a}` and `{b}` already have a road")]
    RoadExists { a: AgentName, b: AgentName },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TakeUpRoad {
    pub a: Named,
    pub b: Named,
    /// Why, for the record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

impl ChiefRequest for TakeUpRoad {
    const NAME: &'static str = "chief_take_up_road";
    type Answer = Done<TakeUpRefusal>;
}

impl ToolCall for TakeUpRoad {
    const TOOL: SocietyTool = SocietyTool::TakeUpRoad;
    type Answer = RoadDone<LeadRefusal<TakeUpRefusal>>;
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
    #[error("no agent has the address `{address}`")]
    NoSuchAgent { address: String },
    #[error("`{a}` and `{b}` have no road to take up")]
    NoRoad { a: AgentName, b: AgentName },
}

/// Asks the caller's lead for a road to `to`, as a letter to the
/// lead.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestRoad {
    pub to: Named,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
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
    #[error("no agent has the address `{address}`")]
    NoSuchAgent { address: String },
    #[error("a road joins hires only")]
    NoLead,
    #[error("your lead `{lead}` is still being hired")]
    StillHiring { lead: AgentName },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetTools {
    pub hire: Named,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub off: Vec<ToolName>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub on: Vec<ToolName>,
    /// Why, for the record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

impl ChiefRequest for SetTools {
    const NAME: &'static str = "chief_set_tools";
    type Answer = Done<OnlyAHire>;
}

impl ToolCall for SetTools {
    const TOOL: SocietyTool = SocietyTool::SetTools;
    type Answer = DoneTo<LeadRefusal<OnlyAHire>>;
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
    #[error("no hamlet has the id `{}`", id.0)]
    NoSuchHamlet { id: HamletId },
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
    /// Set in order, so the last of one kind holds.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub limits: Vec<Limit>,
}

/// One of a hamlet's limits beside its budget, or the realm's
/// `roads_between`, as a chief sets it (H22). A hamlet's model is set
/// with [`choices::SetModel`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Limit {
    /// The most times an agent asks a chief in one turn.
    Questions {
        most: u32,
    },
    /// The most calls of one turn a chief refuses.
    Refusals {
        most: u32,
    },
    Review {
        review: Review,
    },
    /// How long after a chief's task a lead may not end the hire
    /// (H7).
    ProtectedMinutes {
        minutes: u32,
    },
    /// How many days of decisions each hire is told (H17).
    ToldDays {
        days: u32,
    },
    /// Whether a lead's act waits for a chief.
    Reserved {
        act: ReservedAct,
        reserved: bool,
    },
    /// What a road costs, in millionths of a dollar (H3).
    RoadPrice {
        micros: u64,
    },
    /// The most roads a hamlet holds.
    RoadsMost {
        most: u32,
    },
    RoadsGrowOverDays {
        days: u32,
    },
    /// The realm's own: whether mayors may agree roads between their
    /// hamlets.
    RoadsBetween {
        allowed: bool,
    },
}

/// When a lead reviews a chief's instruction to a hire under it
/// (H10).
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Review {
    Always,
    /// When the lead's model is served from another machine.
    Remote,
    Never,
}

/// A lead's act a chief may keep for itself.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ReservedAct {
    End,
    Hire,
    Merge,
    Roads,
}

impl ChiefRequest for SetLimits {
    const NAME: &'static str = "chief_set_limits";
    type Answer = Done<HamletRefusal>;
}

/// The refusal of a chief's act on a hamlet.
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
    #[error("no hamlet has the id `{}`", id.0)]
    NoSuchHamlet { id: HamletId },
    #[error("there is no file `{}` here", path.display())]
    NoSuchFolder { path: PathBuf },
    #[error(
        "`{name}` is not a name: use one line of 1 to 40 characters \
         with no colon"
    )]
    BadName { name: String },
    /// A site that is not a spot in the city, as `x, z`.
    #[error("`{site}` is not an address")]
    BadSite { site: String },
    #[error("Those limits do not fit their form.")]
    MisshapenLimits,
}
