//! The calls the `workspace` actor takes, and its answers.
//!
//! Every answer is built from the same few arms:
//!
//! - `ended { end }`: the call ended, and `end` says how. It is the
//!   end [`crate::agent::Finished`] carries. A repeat of the call
//!   answers it again, except a change that `failed` before it was
//!   planned, as an edit whose text is not found: the workspace kept
//!   nothing of it.
//! - `running`, `waits_for_chief`: the call goes on, and the agent is
//!   called with `Finished` when it ends.
//! - `refused { why, words }`: the workspace kept nothing of the
//!   call. `why` is one of the families of [`refusal`].

pub mod files;
pub mod reach;
pub mod refusal;

use std::path::PathBuf;

use hamlet_actor::Action;
use serde::{Deserialize, Serialize};

use self::reach::{Need, Reach};
use self::refusal::{ChangeNotRun, RunNotRun, RunRefusal};
use crate::address::{Agent, Realm, Workspace};
use crate::{Message, worded};

/// The number in an agent actor's file name,
/// `actors/agent/<n>.sqlite`, and its key.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
)]
pub struct AgentNumber(pub u32);

/// The id an agent gives a tool call. A repeat of the call carries
/// the same id.
#[derive(
    Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub struct CallId(pub String);

/// A conversation of an agent, which trust is given for (H11).
#[derive(
    Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub struct ConversationId(pub String);

/// The realm's task a call is made for, which counts a chief's
/// refusals.
#[derive(
    Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub struct RealmTask(pub String);

/// The realm's version of one agent's reach, from 1.
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
pub struct ReachVersion(pub u64);

/// Sets an agent's reach when `version` is newer than the one in
/// force. Called by the realm.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetReach {
    pub agent: AgentNumber,
    pub version: ReachVersion,
    pub reach: Reach,
}

impl Action for SetReach {
    type Output = InForce;
    const NAME: &'static str = "set_reach";
}

impl Message for SetReach {
    type To = Workspace;
    type From = Realm;
}

/// The workspace's answer to a [`SetReach`]: the agent's version in
/// force after the call.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InForce {
    pub version: ReachVersion,
}

/// A chief's answer to a call that waits for one, or the end of the
/// wait (H11, H34). Called by the realm. `Allow` allows this call;
/// for an answer that allows more than once, the realm sets the reach
/// first and has its answer before it calls.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChiefAnswer {
    pub agent: AgentNumber,
    pub call: CallId,
    pub answer: Answer,
}

impl Action for ChiefAnswer {
    type Output = AnswerTaken;
    const NAME: &'static str = "chief_answer";
}

impl Message for ChiefAnswer {
    type To = Workspace;
    type From = Realm;
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Answer {
    Allow,
    Refuse {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    Lapsed {
        lapse: Lapse,
    },
}

/// The reason a wait ended with no answer (H34).
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Lapse {
    /// A chief stopped the agent's turn.
    Stopped,
    /// The conversation was taken back past the call.
    Rewound,
    /// The agent ended.
    Ended,
}

/// The workspace's answer to a [`ChiefAnswer`]. A wait that ended
/// without the call running answers by the call's kind, with the
/// `why` the agent is called with.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AnswerTaken {
    /// An allow was taken: the command started, or the change was
    /// carried out. The agent is called with its end.
    Taken,
    /// A run did not run: a chief refused it, its wait lapsed, or
    /// its allow no longer holds.
    #[serde(serialize_with = "worded")]
    RunNotRun { why: RunNotRun },
    /// The same, for a write or an edit.
    #[serde(serialize_with = "worded")]
    ChangeNotRun { why: ChangeNotRun },
    /// The agent has no call waiting under this id: its wait ended
    /// before, and nothing changed.
    NoWait,
}

/// Runs a script or a tool added by hand for the calling agent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    pub call: CallId,
    pub conversation: ConversationId,
    /// `None` when no chief's instruction started the work.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub realm_task: Option<RealmTask>,
    pub program: Program,
}

/// The script or tool a [`Run`] runs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Program {
    Script {
        script: String,
        /// The model said the command needs the network (C19).
        network: bool,
        /// Shortens the 120 s limit (C16), in milliseconds.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_ms: Option<u32>,
    },
    /// A tool a chief added by hand (C17), which reads `arguments`,
    /// a JSON object, on its standard input. It runs with the reach
    /// and the time limit its manifest gives.
    Tool { name: String, arguments: String },
}

impl Action for Run {
    type Output = RunAnswer;
    const NAME: &'static str = "run";
}

impl Message for Run {
    type To = Workspace;
    type From = Agent;
}

/// Asks which tools a chief added by hand (C17), to offer them to a
/// model.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListTools;

impl Action for ListTools {
    type Output = ToolList;
    const NAME: &'static str = "list_tools";
}

impl Message for ListTools {
    type To = Workspace;
    type From = Agent;
}

/// The tools added by hand that load, in their folders' order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolList {
    pub tools: Vec<AddedTool>,
    /// Each folder of a tool that does not load.
    pub faults: Vec<Fault>,
}

/// A tool added by hand, as a model is offered it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddedTool {
    pub name: String,
    pub description: String,
    /// The JSON Schema of its arguments, as JSON.
    pub parameters: String,
}

/// A tool's folder, or the tools' own, that is left out of a
/// [`ToolList`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fault {
    /// The folder's path, as text even where its name is not UTF-8.
    pub folder: String,
    /// Why it does not load, in words.
    pub words: String,
}

/// The workspace's id of a command, from 1: its call's row id, which
/// fits in a `u32` or the command is not started.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub struct CommandId(pub u32);

/// The workspace's answer to a [`Run`], and to a [`CommandStatus`]
/// with `R` as [`refusal::StatusRefusal`]: how the call's command
/// stands.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[serde(bound(
    serialize = "R: Serialize + std::fmt::Display",
    deserialize = "R: Deserialize<'de>"
))]
pub enum RunAnswer<R = RunRefusal> {
    Ended {
        end: RunEnd,
    },
    /// The command runs on, and the agent is called with
    /// [`crate::agent::Finished`] when it ends, unless the
    /// workspace's host stopped first.
    Running {
        command: CommandId,
    },
    /// The call waits for a chief to allow what it needs. The agent
    /// is called with [`crate::agent::Finished`] when the wait ends.
    WaitsForChief {
        needs: Vec<Need>,
    },
    #[serde(serialize_with = "worded")]
    Refused {
        why: R,
    },
}

/// How a run ended.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RunEnd {
    /// Its command ended.
    Ran(CommandEnd),
    /// It waited for a chief and did not run.
    #[serde(serialize_with = "worded")]
    NotRun { why: RunNotRun },
}

/// A command's exit, and the end of what it printed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandEnd {
    pub command: CommandId,
    pub exit: Exit,
    /// Its last 2000 lines or 50 KiB, the secrets taken out.
    pub output: String,
    /// Set when `output` is not all it printed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cut: Option<Cut>,
    /// The hash of the sandbox policy it ran under.
    pub policy: String,
    pub took_ms: u64,
}

/// The part of a command's output beyond the tail its end carries.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cut {
    /// All it printed, the secrets taken out.
    pub total_bytes: u64,
    pub total_lines: u64,
    /// The file that holds all it printed, up to 512 MiB, which the
    /// agent may read.
    pub full_output: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Exit {
    /// Its exit code, or 128 and the signal that ended it.
    Code {
        code: i32,
    },
    TimedOut,
    /// It did not start, or its output could not be kept.
    Failed {
        words: String,
    },
    /// The workspace's host stopped or was killed while it ran.
    CutOff,
    /// A [`Cancel`] or a [`CancelAgent`] ended it, or its agent's
    /// reach ended.
    Cancelled,
}

/// Ends the named running commands of an agent, each as
/// [`Exit::Cancelled`]. The realm names the agent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancelAgent {
    pub agent: AgentNumber,
    pub which: Which,
}

impl Action for CancelAgent {
    type Output = Cancelled;
    const NAME: &'static str = "cancel_agent";
}

impl Message for CancelAgent {
    type To = Workspace;
    type From = Realm;
}

/// Ends the named running commands of the calling agent, each as
/// [`Exit::Cancelled`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cancel {
    pub which: Which,
}

impl Action for Cancel {
    type Output = Cancelled;
    const NAME: &'static str = "cancel";
}

impl Message for Cancel {
    type To = Workspace;
    type From = Agent;
}

/// The calls whose commands a [`Cancel`] ends.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Which {
    One { call: CallId },
    Conversation { conversation: ConversationId },
    All,
}

/// The workspace's answer to a [`Cancel`] and to a [`CancelAgent`]:
/// the commands it ended, none when none of them ran.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cancelled {
    pub commands: Vec<CommandId>,
}

/// Asks how a command of the calling agent stands.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandStatus {
    pub command: CommandId,
}

impl Action for CommandStatus {
    type Output = RunAnswer<refusal::StatusRefusal>;
    const NAME: &'static str = "command_status";
}

impl Message for CommandStatus {
    type To = Workspace;
    type From = Agent;
}
