//! The calls the `workspace` actor takes, and its answers.

pub mod files;
pub mod reach;

use std::path::PathBuf;

use hamlet_actor::Action;
use serde::{Deserialize, Serialize};

use self::files::Hashed;
use self::reach::{Need, Reach, Refusal};

/// The number in an agent actor's file name,
/// `actors/agent/<n>.sqlite`, and its key.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub struct AgentNumber(pub u64);

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
    type Output = ReachAnswer;
    const NAME: &'static str = "set_reach";
}

/// The workspace's answer to a [`SetReach`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ReachAnswer {
    /// The agent's version in force after the call.
    InForce {
        version: ReachVersion,
    },
    Refused {
        why: RealmRefusal,
    },
}

/// A call only the realm makes, which the workspace does not take
/// from this caller.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
#[serde(rename_all = "snake_case")]
pub enum RealmRefusal {
    #[error("only the realm makes this call")]
    NotTheRealm,
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

/// The workspace's answer to a [`ChiefAnswer`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AnswerTaken {
    /// The call runs, and the agent is called with its end.
    Runs,
    /// The call does not run, and the agent is called with why.
    NotRun {
        why: CallRefusal,
    },
    /// The agent has no call waiting under this id: its wait ended
    /// before, and nothing changed.
    NoWait,
    Refused {
        why: RealmRefusal,
    },
}

/// A call of an agent's tool from a caller it is not for.
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
pub enum CallerRefusal {
    #[error("only an agent calls for its tools")]
    NotAnAgent,
    #[error("agent {} called for agent {}", caller.0, named.0)]
    NotTheCaller {
        named: AgentNumber,
        caller: AgentNumber,
    },
}

/// Runs a script or a tool added by hand for an agent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    pub agent: AgentNumber,
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
        timeout_ms: Option<u64>,
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

/// Asks which tools a chief added by hand (C17), to offer them to a
/// model.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListTools;

impl Action for ListTools {
    type Output = ToolList;
    const NAME: &'static str = "list_tools";
}

/// The tools added by hand that load, in their folders' order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolList {
    pub tools: Vec<AddedTool>,
    /// The reason each folder of a tool that does not load is left
    /// out.
    pub faults: Vec<String>,
}

/// A tool added by hand, as a model is offered it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddedTool {
    pub name: String,
    pub description: String,
    /// The JSON Schema of its arguments, as JSON.
    pub parameters: String,
}

/// The workspace's id of a command, from 1.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub struct CommandId(pub u64);

/// The workspace's answer to a [`Run`] and to a [`CommandStatus`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RunAnswer {
    Done(CommandEnd),
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
    Refused {
        why: CallRefusal,
    },
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
    /// A [`Cancel`] ended it, or its agent's reach ended.
    Cancelled,
}

/// Ends the named running commands of an agent, each as
/// [`Exit::Cancelled`]. Called by the realm, or by the agent named.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cancel {
    pub agent: AgentNumber,
    pub which: Which,
}

impl Action for Cancel {
    type Output = CancelAnswer;
    const NAME: &'static str = "cancel";
}

/// The calls whose commands a [`Cancel`] ends.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Which {
    One { call: CallId },
    Conversation { conversation: ConversationId },
    All,
}

/// The workspace's answer to a [`Cancel`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CancelAnswer {
    /// The commands it ended, none when none of them ran.
    Cancelled {
        commands: Vec<CommandId>,
    },
    Refused {
        why: CallerRefusal,
    },
}

/// Asks how a command of the calling agent stands.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandStatus {
    pub agent: AgentNumber,
    pub command: CommandId,
}

impl Action for CommandStatus {
    type Output = RunAnswer;
    const NAME: &'static str = "command_status";
}

/// A call of an agent's tool the workspace does not carry out.
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
pub enum CallRefusal {
    #[error("{why}")]
    Caller { why: CallerRefusal },
    #[error("{why}")]
    Reach { why: Refusal },
    /// The id was given before to a call with another request (S19).
    #[error("the call id {} was used before for another request", call.0)]
    CallReused { call: CallId },
    /// A call that waits for a chief would keep one of the realm's
    /// secrets in its text, which is never stored.
    #[error(
        "the call holds one of the realm's secrets, so it cannot \
         wait for a chief"
    )]
    HoldsSecret,
    #[error("a chief refused the call{}", reason.as_ref().map_or_else(String::new, |reason| format!(": {reason}")))]
    ChiefRefused {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    #[error("the call's wait for a chief lapsed: {}", match lapse {
        Lapse::Stopped => "the turn was stopped",
        Lapse::Rewound => "the conversation was taken back past it",
        Lapse::Ended => "the agent ended",
    })]
    Lapsed { lapse: Lapse },
    /// A chief allowed the call, but the reach changed while it
    /// waited and no longer lets it run.
    #[error(
        "a chief allowed the call under reach version {}, but under \
         version {} {why}",
        since.0,
        now.0
    )]
    ReachChanged {
        since: ReachVersion,
        now: ReachVersion,
        why: Changed,
    },
    #[error("{words}")]
    Invalid { words: String },
    #[error("this agent has no command {}", command.0)]
    NoSuchCommand { command: CommandId },
    /// The file no longer holds what it held when the agent asked,
    /// so the change was not made.
    #[error(
        "{} changed after the change was asked for: it held {was} and \
         now holds {now}. Read it again and ask again",
        path.display()
    )]
    FileChanged {
        path: PathBuf,
        was: Hashed,
        now: Hashed,
    },
    /// A write's content is past [`files::MOST_CONTENT`].
    #[error(
        "the content is {bytes} bytes, more than the {limit} bytes \
         a write may hold"
    )]
    TooLarge { bytes: u64, limit: u64 },
}

/// The reach in force's answer to a call a chief allowed under an
/// older one.
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
pub enum Changed {
    #[error("it is refused: {why}")]
    Refused { why: Refusal },
    /// It needs what the chief was not asked for.
    #[error("it needs {needs:?}, which the chief was not asked for")]
    Needs { needs: Vec<Need> },
}
