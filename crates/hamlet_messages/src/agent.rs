//! The calls an `agent` actor takes from the other actors, and their
//! answers.

pub mod chat;
pub mod copies;

use std::path::PathBuf;

use hamlet_actor::Action;
use hamlet_protocol::client::{AttachedFile, AttachedImage};
use serde::{Deserialize, Serialize};

use self::copies::Copies;
use crate::Message;
use crate::address::{Agent, Realm, Workspace};
use crate::profile::{CommitId, Piece};
use crate::realm::parcels::ParcelFile;
use crate::realm::{LetterId, ProfileId, ToolName};
use crate::workspace::files::ChangeEnd;
use crate::workspace::{
    AgentNumber, CallId, ConversationId, RealmTask, RunEnd,
};

/// Tells the agent how a call ended that the workspace answered
/// `Running` or `WaitsForChief` for. Called by the workspace once,
/// when it ends, unless the workspace's host stopped first. A call
/// that fails is not made again.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finished {
    pub call: CallId,
    /// The realm's task the call was made under, which the turn its
    /// end starts is charged to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<RealmTask>,
    pub outcome: Outcome,
}

impl Action for Finished {
    type Output = ();
    const NAME: &'static str = "finished";
}

impl Message for Finished {
    type To = Agent;
    type From = Workspace;
}

/// The end of a call, by its kind: the `end` its answer would have
/// held under `ended`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Outcome {
    Run { end: RunEnd },
    Change { end: ChangeEnd },
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum AgentKind {
    Hand,
    Hire,
    Worker,
    Errand,
}

/// The input `agent/<n>` is created with.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AgentInput {
    pub kind: AgentKind,
    /// The task that pays for a turn no chief's line started.
    pub task: RealmTask,
    pub place: Place,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brief: Option<Brief>,
    /// Every part the realm holds for the agent, at its version.
    pub copies: Copies,
}

/// The one turn a worker or an errand runs when it is created.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Brief {
    /// An errand's whole prompt (C11); absent for a worker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    /// The turn's input.
    pub text: String,
    /// The built-in tools it may use. Empty is every built-in for a
    /// worker, and none for an errand.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<BuiltIn>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The errand is a dream, which asks its model as a dream's
    /// conversation does, not as another errand's.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dream: bool,
}

/// One of an agent's built-in tools, by name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuiltIn {
    pub name: String,
}

/// The id an input is taken under. A second [`Submit`] with the same
/// id is the first one.
#[derive(
    Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub struct InputId(pub String);

/// An image of a chief's line, kept under the agent's `inbox/images/`
/// and given to its model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LineImage {
    pub image: AttachedImage,
    pub file: ParcelFile,
}

/// A file of a chief's line, kept under the agent's `inbox/`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LineFile {
    pub attached: AttachedFile,
    pub file: ParcelFile,
}

/// Gives the agent an input to work on.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Submit {
    pub id: InputId,
    /// The realm's task the turn is charged to; absent for a letter
    /// whose sender's turn had none, which the standing task pays.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<RealmTask>,
    pub input: Input,
}

impl Action for Submit {
    type Output = ();
    const NAME: &'static str = "submit";
}

impl Message for Submit {
    type To = Agent;
    type From = Realm;
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Input {
    /// A chief's line, with its attachments as the client sent them,
    /// each in a parcel for the agent to fetch.
    Line {
        text: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        images: Vec<LineImage>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        files: Vec<LineFile>,
        /// The note of the lead who reviewed the line.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
        /// The name of the chief who said it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        chief: Option<String>,
    },
    /// A chief's ruling, or news the realm tells the agent.
    Notice {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        conversation: Option<ConversationId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        held: Option<HeldCall>,
    },
    /// The answer of a worker the agent started, for the conversation
    /// it was started from; absent when its start named none.
    WorkerAnswer {
        from: Author,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        conversation: Option<ConversationId>,
    },
    Letter {
        from: Author,
        text: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        files: Vec<ParcelFile>,
    },
    /// A letter the agent answers with
    /// [`crate::realm::letters::Reply`] when its turn on it ends.
    Question {
        letter: LetterId,
        from: Author,
        text: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        files: Vec<ParcelFile>,
    },
    /// The answer to a question the agent asked, for the conversation
    /// it asked from.
    Answer {
        question: LetterId,
        from: Author,
        text: String,
        conversation: ConversationId,
    },
}

/// A society call the realm kept for a chief, which a ruling or a
/// stop ends.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeldCall {
    pub call: CallId,
    pub end: HeldEnd,
}

/// How a held call ended.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum HeldEnd {
    /// The chief allowed or answered it, or told the agent to go on.
    Ran,
    /// It did not run; the notice says why.
    Blocked,
    /// A stop ended it.
    Aborted,
    /// The chief withdrew the instruction, as the notice says.
    Withdrawn,
}

/// The writer of a letter, as the receiver is shown it (C30).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Author {
    pub agent: AgentNumber,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// The realm's version of one agent's place, from 1.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
)]
pub struct PlaceVersion(pub u64);

/// An agent's place in the tree.
/// The agent keeps it when `version` is newer than the one it holds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Place {
    pub version: PlaceVersion,
    /// Absent for the Hand and a mayor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead: Option<AgentNumber>,
    pub hires: Vec<AgentNumber>,
    /// The agents it has a road to, its lead and its hires left out.
    pub roads: Vec<AgentNumber>,
    pub tools_off: Vec<ToolName>,
    /// Its hamlet's project folder, where its commands run and a
    /// relative path starts; absent when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<PathBuf>,
    /// Whether its hamlet runs in manual mode.
    #[serde(default)]
    pub manual: bool,
    /// Whether it is a mayor while another hamlet lives, so it may
    /// agree a road with another hamlet's mayor.
    #[serde(default)]
    pub agrees_roads: bool,
}

impl Action for Place {
    type Output = ();
    const NAME: &'static str = "place";
}

impl Message for Place {
    type To = Agent;
    type From = Realm;
}

/// Fills the agent's filesystem from its branch of `profile`. An
/// agent already filled pulls its branch's tip between turns, and
/// answers at once.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fill {
    pub profile: ProfileId,
}

impl Action for Fill {
    type Output = ();
    const NAME: &'static str = "fill";
}

impl Message for Fill {
    type To = Agent;
    type From = Realm;
}

/// Ends the agent's work for its end: it stops its turn as a stop
/// does, then saves what its filesystem changed to its branch. A
/// repeat saves what changed since.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Retire;

/// The branch's tip once the agent's changes are saved on it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Retired {
    pub tip: CommitId,
}

impl Action for Retire {
    type Output = Retired;
    const NAME: &'static str = "retire";
}

impl Message for Retire {
    type To = Agent;
    type From = Realm;
}

/// Erases the agent: it answers, then asks its host to destroy it,
/// which removes its database and its filesystem. A repeat to an
/// agent already gone finds none.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Erase;

impl Action for Erase {
    type Output = ();
    const NAME: &'static str = "erase";
}

impl Message for Erase {
    type To = Agent;
    type From = Realm;
}

/// Reads one part of the files that wait in the agent's `inbox/`, in
/// path order from `after`: an absorbed hire's, for the hire it was
/// merged into. Each piece's path is from `inbox/`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadInbox {
    /// The `next` of the part before; absent for the first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<InboxCursor>,
}

/// Where the next part of an inbox starts: at the first file whose
/// path is `path` or after it, from its byte `offset`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InboxCursor {
    pub path: String,
    pub offset: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InboxPart {
    pub files: Vec<Piece>,
    /// Absent on the last part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<InboxCursor>,
}

impl Action for ReadInbox {
    type Output = InboxPart;
    const NAME: &'static str = "read_inbox";
}

impl Message for ReadInbox {
    type To = Agent;
    type From = Realm;
}

/// Writes `files`, one part of the inbox of the hire `from` that was
/// merged into this agent's, under `inbox/from-hire-<from>/`, over a
/// file of the same path. A file's pieces come in order from offset
/// 0, and it is written once its last piece arrives. It gives no
/// turn.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TakeInbox {
    pub from: AgentNumber,
    pub files: Vec<Piece>,
}

impl Action for TakeInbox {
    type Output = ();
    const NAME: &'static str = "take_inbox";
}

impl Message for TakeInbox {
    type To = Agent;
    type From = Realm;
}

/// Removes `paths`, each from `inbox/`, once the hire this one was
/// merged into has taken them. A path already gone is removed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DropInbox {
    pub paths: Vec<String>,
}

impl Action for DropInbox {
    type Output = ();
    const NAME: &'static str = "drop_inbox";
}

impl Message for DropInbox {
    type To = Agent;
    type From = Realm;
}

/// Asks whether the agent works on one of the chief's tasks `tasks`:
/// an input under one is still to end, or one ended less than
/// `within_ms` ago. The realm asks it before a lead ends or merges
/// the hire (H7).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OnTask {
    pub tasks: Vec<RealmTask>,
    pub within_ms: u64,
}

impl Action for OnTask {
    type Output = bool;
    const NAME: &'static str = "on_task";
}

impl Message for OnTask {
    type To = Agent;
    type From = Realm;
}
