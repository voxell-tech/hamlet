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
/// refusals. Not the id of the call's own durable run.
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
pub enum ReachAnswer {
    /// The agent's version in force after the call.
    InForce(ReachVersion),
    Refused(ReachRefusal),
}

/// A [`SetReach`] the workspace does not take.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
pub enum ReachRefusal {
    #[error("only the realm sets a reach")]
    NotTheRealm,
}

/// A call of an agent's tool from a caller it is not for (W2).
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    thiserror::Error,
)]
pub enum CallerRefusal {
    #[error("only an agent calls for its tools")]
    NotAnAgent,
    #[error("agent {} called for agent {}", caller.0, named.0)]
    NotTheCaller {
        named: AgentNumber,
        caller: AgentNumber,
    },
}

/// The shell a command's script is written for.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
pub enum Shell {
    Bash,
}

/// Runs a script for an agent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    pub agent: AgentNumber,
    pub call: CallId,
    pub conversation: ConversationId,
    /// `None` when no chief's instruction started the work.
    pub realm_task: Option<RealmTask>,
    pub shell: Shell,
    pub script: String,
    /// The model said the command needs the network (C19).
    pub network: bool,
    /// Shortens the 120 s limit (C16), in milliseconds.
    pub timeout_ms: Option<u64>,
}

impl Action for Run {
    type Output = RunAnswer;
    const NAME: &'static str = "run";
}

/// The workspace's id of a command, from 1.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub struct CommandId(pub u64);

/// The workspace's answer to a [`Run`] and to a [`CommandStatus`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunAnswer {
    Done(CommandEnd),
    /// The command runs on, and the agent is called with
    /// [`crate::agent::Finished`] when it ends.
    Running {
        command: CommandId,
    },
    Refused(CallRefusal),
}

/// How a command ended, and the end of what it printed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandEnd {
    pub command: CommandId,
    pub exit: Exit,
    /// Its last 2000 lines or 50 KiB, the secrets taken out.
    pub output: String,
    /// Set when `output` is not all it printed.
    pub cut: Option<Cut>,
    /// The hash of the sandbox policy it ran under.
    pub policy: String,
    pub took_ms: u64,
}

/// What a command printed beyond the tail its end carries.
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
pub enum Exit {
    /// Its exit code, or 128 and the signal that ended it.
    Code(i32),
    TimedOut,
    /// It did not start, or its output could not be kept.
    Failed(String),
    /// The workspace's host stopped or was killed while it ran.
    CutOff,
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
pub enum CallRefusal {
    #[error(transparent)]
    Caller(CallerRefusal),
    #[error(transparent)]
    Reach(Refusal),
    /// The id was given before to a call with another request (S19).
    #[error("the call id {} was used before for another request", .0.0)]
    CallReused(CallId),
    #[error(
        "this call needs a chief to allow {0:?}, and the workspace \
         runs only what needs no chief"
    )]
    NeedsChief(Vec<Need>),
    #[error("{0}")]
    Invalid(String),
    #[error("this agent has no command {}", .0.0)]
    NoSuchCommand(CommandId),
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
